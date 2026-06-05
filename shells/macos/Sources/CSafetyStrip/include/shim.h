/* CSafetyStrip shim header.
 *
 * Single source of truth for the C ABI is the cbindgen-generated header in the
 * repo at `core-ffi/include/safetystrip.h`. Rather than copy it (which would
 * risk drift from the frozen original), we include it by relative path.
 *
 * Path math: this file lives at
 *   shells/macos/Sources/CSafetyStrip/include/shim.h
 * and the header lives at
 *   core-ffi/include/safetystrip.h
 * so we climb five `..` (include -> CSafetyStrip -> Sources -> macos -> shells)
 * to reach the repo root, then descend into core-ffi/include.
 */
#include "../../../../../core-ffi/include/safetystrip.h"
