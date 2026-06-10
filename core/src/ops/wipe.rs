//! Wipe-before-growth appends for clipboard-derived output accumulators.
//!
//! A plain `String::push`/`push_str` that outgrows its capacity reallocates and
//! hands the old block back to the allocator with its clipboard-derived bytes
//! intact — the `Zeroizing` wrapper an op output later gains (in the pipeline)
//! only wipes the *final* allocation on drop, never the ones a mid-construction
//! reallocation already freed.
//!
//! Most ops avoid the problem by pre-sizing: their output is shrink-or-equal or
//! has a cheap provably-sufficient bound, so the single allocation never moves
//! (see the per-op `with_capacity` comments). The ops whose output can outgrow
//! any cheap up-front bound (`html_to_markdown`, the Unicode case mappings)
//! instead route every append through these helpers: when an append must grow
//! the buffer, the bytes are moved by hand and the superseded allocation is
//! zeroized **before** it returns to the allocator — the same posture as the
//! pipeline's fused-scratch `prepare_collapse_scratch`, adapted for accumulators
//! whose contents must survive the growth.
//!
//! Best-effort, like the rest of the wipe posture: it covers the heap blocks we
//! own; it cannot cover allocator metadata, registers, or OS paging.

use zeroize::Zeroizing;

/// Append `s` to `buf`, wiping the superseded allocation if the append grows it.
pub(crate) fn push_str_wiping(buf: &mut String, s: &str) {
    reserve_wiping(buf, s.len());
    buf.push_str(s);
}

/// Append `c` to `buf`, wiping the superseded allocation if the append grows it.
pub(crate) fn push_char_wiping(buf: &mut String, c: char) {
    reserve_wiping(buf, c.len_utf8());
    buf.push(c);
}

/// Make room for `additional` more bytes without ever letting `String` itself
/// reallocate (which would free the old block unwiped).
///
/// Growth at least doubles the capacity so appends stay amortized O(1) — the
/// move-and-wipe below is O(len), and doubling bounds the total moved bytes by
/// O(final length), keeping every caller linear-time on adversarial input.
pub(crate) fn reserve_wiping(buf: &mut String, additional: usize) {
    let needed = buf.len().saturating_add(additional);
    if needed <= buf.capacity() {
        return;
    }
    let mut grown = String::with_capacity(needed.max(buf.capacity().saturating_mul(2)));
    grown.push_str(buf);
    let retired = std::mem::replace(buf, grown);
    // `Zeroizing` wipes the retired block's full capacity before it is freed, so
    // no clipboard-derived bytes linger in allocator-owned memory.
    drop(Zeroizing::new(retired));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_preserve_content_across_growth() {
        // Start deliberately undersized so every append path exercises growth.
        let mut buf = String::with_capacity(1);
        let mut expected = String::new();
        for i in 0u8..64 {
            push_str_wiping(&mut buf, "ab");
            push_char_wiping(&mut buf, char::from(b'0' + (i % 10)));
            expected.push_str("ab");
            expected.push(char::from(b'0' + (i % 10)));
        }
        assert_eq!(buf, expected);
    }

    #[test]
    fn reserve_never_lets_the_next_append_reallocate() {
        let mut buf = String::new();
        for chunk in ["short", &"x".repeat(100), &"y".repeat(1000)] {
            reserve_wiping(&mut buf, chunk.len());
            let cap_before = buf.capacity();
            buf.push_str(chunk);
            assert_eq!(
                buf.capacity(),
                cap_before,
                "append after reserve_wiping must not move the buffer"
            );
        }
    }
}
