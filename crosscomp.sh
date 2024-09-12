#!/bin/bash

export OPENSSL_DIR="/usr/bin/openssl"
export OPENSSL_INCLUDE_DIR="/usr/include/openssl"

if [[ "$1" == "arm" ]] || [[ -z "$1" ]]; then
  echo "building Arm"
  cargo build --target armv7-linux-androideabi
elif [[ "$1" == "arm64" ]] || [[ -z "$1" ]]; then
  echo "building Arm64"
  cargo build --target aarch64-linux-android
elif [[ "$1" == "x86" ]] || [[ -z "$1" ]]; then
  echo "building X86"
  cargo build --target i686-linux-android
elif [[ "$1" == "x86_64" ]] || [[ -z "$1" ]]; then
  echo "building X86_64"
  cargo build --target x86_64-linux-android
fi
