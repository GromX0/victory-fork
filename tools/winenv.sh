# Source from the victory-fork code checkout: cross-build to x86_64-pc-windows-msvc with clang-cl/lld-link + xwin, tests under Wine.
export PATH=$HOME/.local/xbin:$PATH
SDK=$HOME/.local/share/xwin-sdk
export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=lld-link
export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS="-Clink-arg=/winsysroot:$SDK"
export CC_x86_64_pc_windows_msvc=clang-cl CXX_x86_64_pc_windows_msvc=clang-cl AR_x86_64_pc_windows_msvc=llvm-lib
export CFLAGS_x86_64_pc_windows_msvc="/winsysroot $SDK -Wno-unused-command-line-argument"
export CXXFLAGS_x86_64_pc_windows_msvc="$CFLAGS_x86_64_pc_windows_msvc"
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)/target}   # the checkout you sourced this from (source it from the victory-fork checkout)
# Ubuntu's Wine 9.0 aborts at startup ("free(): invalid pointer") while TMPDIR is set.
unset TMPDIR
export WINEDEBUG=${WINEDEBUG:--all}
