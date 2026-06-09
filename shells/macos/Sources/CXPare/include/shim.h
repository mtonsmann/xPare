/* CXPare shim header.
 *
 * Single source of truth for the C ABI is the cbindgen-generated header in the
 * repo at `core-ffi/include/xpare.h`. Rather than copy it (which would
 * risk drift from the frozen original), we include it by relative path.
 *
 * Path math: this file lives at
 *   shells/macos/Sources/CXPare/include/shim.h
 * and the header lives at
 *   core-ffi/include/xpare.h
 * so we climb five `..` (include -> CXPare -> Sources -> macos -> shells)
 * to reach the repo root, then descend into core-ffi/include.
 */
#include "../../../../../core-ffi/include/xpare.h"
