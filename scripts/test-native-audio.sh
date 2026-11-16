#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
meeting_test_dir=$(mktemp -d "${TMPDIR:-/tmp}/meeting-pcm-tests.XXXXXX")
trap 'rm -r "$meeting_test_dir"' EXIT HUP INT TERM
meeting_sdk=$(xcrun --sdk macosx --show-sdk-path)
meeting_sanitizers=${MEETING_NATIVE_SANITIZERS:-address,undefined}
xcrun clang -std=c11 -O1 -g -Wall -Wextra -Werror \
  "-fsanitize=$meeting_sanitizers" -fno-omit-frame-pointer \
  -isysroot "$meeting_sdk" -mmacosx-version-min=14.2 \
  -I apps/desktop/native apps/desktop/native/PCMQueue.c \
  apps/desktop/tests/pcm_queue_test.c -lpthread -lm \
  -o "$meeting_test_dir/pcm_queue_test"
# Apple ASan supports bounds/use-after-free checks here, but not LeakSanitizer.
ASAN_OPTIONS=detect_leaks=0 UBSAN_OPTIONS=halt_on_error=1 TSAN_OPTIONS=halt_on_error=1 \
  "$meeting_test_dir/pcm_queue_test"
xcrun swiftc -swift-version 6 -O \
  apps/desktop/native/AudioWatchdog.swift apps/desktop/tests/audio_watchdog_test.swift \
  -o "$meeting_test_dir/audio_watchdog_test"
"$meeting_test_dir/audio_watchdog_test"
xcrun swiftc -swift-version 6 -O \
  apps/desktop/native/VoiceIsolation.swift apps/desktop/tests/voice_isolation_test.swift \
  -o "$meeting_test_dir/voice_isolation_test"
# Speech made on this Mac, so the timeline check needs no audio in the repository.
say --file-format=WAVE --data-format=LEF32@48000 -o "$meeting_test_dir/speech.wav" \
  "Thanks for joining. Let's review the roadmap and decide which features ship first."
"$meeting_test_dir/voice_isolation_test" "$meeting_test_dir/speech.wav"
