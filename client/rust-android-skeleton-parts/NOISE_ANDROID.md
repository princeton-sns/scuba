# Android

Install 4 android targets via rustup: `x86_64-linux-android`, `i686-linux-android`,
`aarch64-linux-android`, and `arm7-linux-androideabi`.

Make sure SDK and NDK are installed (if not, install them) - via SDK manager.

```sh
cd noise-rust/client/rust-android-skeleton-parts/
```

Import/copy the missing parts of `build.gradle` and `app/build.gradle` into their
respective locations in your Android Studio project.

From `app/src/main/java/com/example/rustskeleton/MainActivity.kt` copy lines 24-34
into your project's driver class file, replacing `rust` on line 32 with the name
of the library you'd like to link (`noise-kv` in my case).

Then copy the code from `rust/src/lib.rs` into your library's `lib.rs` file, 
replacing `helloRust` with a more relevant/descriptive name pertaining to your
project (in both the `lib.rs` and in the `MainActivity.kt` file, at line 28 in 
the skeleton one). Also replace `rustskeleton` in the `lib.rs` with the 
lower-case version of your Android Studio project name (mine is 
`noiseperiodtracker`), for the full extern function's name to look something like
`Java_com_example_noiseperiodtracker_MainActivity_createStandaloneDevice` instead
of `Java_com_example_rustskeleton_MainActivity_helloRust`.

Then try building. You may run into an openssl/include error that we fixed by 
setting the following environment variable: 

`export OPENSSL_INCLUDE_DIR="/usr/include/openssl/"`

We then ran into a second error about ring's custom build command failing, which
is where we are currently blocked.

The first error in the log (for this single build failure) is:

```sh
failed to run custom build command for `openssl-sys v0.9.87`
```

whereas the second error in the log is:

```sh
failed to run custom build command for `ring v0.16.20`
```

Trying to muck around with the openssl version resulted in some weird caching 
issues, where once I removed the explicit openssl version (which isn't usually
a direct dependency, I just added it as one to be able to control the version
number although I don't know if that successfully affects all openssl versions 
in the dependency tree) the dependency did not return to the previous openssl
version and the build also failed claiming that that package does not exist.

After mucking around with various `clean`s and `rm Cargo.lock` in the workspace
root dir, the version returned to the original one but the build continues to
fail with the above error.

Re-building from a clean git clone does not persist the issue, so that's a 
workaround, but still don't know why this was happening.
