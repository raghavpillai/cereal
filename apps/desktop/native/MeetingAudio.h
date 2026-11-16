/*
MIT License

Copyright (c) 2026 OpenOats contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

*/

#ifndef MEETING_AUDIO_H
#define MEETING_AUDIO_H
#include <stdint.h>
#include <stddef.h>
#ifdef __cplusplus
extern "C" {
#endif
/* flags: microphone=1, system output=2, levels only (no files)=4, voice isolation
 * on the microphone=8. macOS 14.2+. All paths UTF-8.
 * start returns immediately; inspect independent channel states through poll.
 * JSON methods return required bytes INCLUDING NUL; insufficient buffers are left empty.
 * stop is synchronous, idempotent, cancels startup, drains writers, and keeps final stats.
 * destroy must be called exactly once, with no concurrent API calls on that handle. */
void *mn_audio_start(const char *output_directory, uint32_t channel_flags, char *error, size_t error_capacity);
size_t mn_audio_poll(void *handle, char *json, size_t capacity);
size_t mn_audio_stop(void *handle, char *json, size_t capacity);
void mn_audio_destroy(void *handle);
#ifdef __cplusplus
}
#endif
#endif
