# Execution Plan: macOS OCR Review Closure

## Scope

Change class: macOS shell, pasteboard race handling, review-finding closure.

Close the review finding class where continuous mode can read a pasteboard generation,
then process stale text or image bytes after the live pasteboard changed.

## Decisions

- Treat the root issue as a post-read TOCTOU across automatic pasteboard reads, not
  only as an OCR writeback problem.
- Re-check the live generation and nspasteboard.org do-not-process marker after
  text/image materialization and before running the core or Vision.
- Keep the core ABI unchanged; pasteboard ordering is a shell responsibility.

## Evidence

- Review finding class: automatic mode trusted a generation captured before content
  materialization.
- Regression protection: Swift tests simulate marker/generation changes during text
  and image reads before the core or Vision can process stale content.
- Docs lesson: macOS posture now requires post-read generation and marker checks before
  automatic transform or Vision work.
