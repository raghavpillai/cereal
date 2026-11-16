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

#include "PCMQueue.h"
#include <stdatomic.h>
#include <stdlib.h>
#include <stdbool.h>
#include <string.h>
#include <math.h>
#include <time.h>

/* Frames the level is measured over: one microphone callback, about 90 ms. */
#define MN_LEVEL_WINDOW_FRAMES 4096u

/* Bounded single-producer/single-consumer storage, allocated before capture starts.
 * Audio callbacks never allocate, wait, dispatch work, or write to disk. */
struct MNPCMQueue {
    uint32_t slots, capacity;
    float *samples;
    uint32_t *lengths;
    _Atomic uint64_t write_index, read_index;
    _Atomic uint64_t received, written, dropped;
    _Atomic float level;
    /* Producer-only: the squares and frames gathered towards the next level. */
    double level_squares;
    uint32_t level_frames;
    _Atomic double uptime;
    _Atomic bool accepting;
    _Atomic uint32_t invalid_format;
    _Atomic uint32_t callbacks;
};

MNPCMQueue *mn_pcm_create(uint32_t slots, uint32_t capacity) {
    if (slots < 2 || slots > 128 || capacity == 0 || capacity > 65536) return NULL;
    MNPCMQueue *q = calloc(1, sizeof(*q));
    if (!q) return NULL;
    q->samples = calloc((size_t)slots * capacity, sizeof(float));
    q->lengths = calloc(slots, sizeof(uint32_t));
    if (!q->samples || !q->lengths) { mn_pcm_destroy(q); return NULL; }
    q->slots = slots; q->capacity = capacity;
    atomic_store(&q->accepting, true);
    return q;
}
void mn_pcm_destroy(MNPCMQueue *q) { if (q) { free(q->samples); free(q->lengths); free(q); } }
void mn_pcm_close(MNPCMQueue *q) { if(q) { atomic_store(&q->accepting, false); atomic_store(&q->level, 0); } }
uint32_t mn_pcm_callbacks_active(MNPCMQueue *q) { return q ? atomic_load(&q->callbacks) : 0; }
double mn_pcm_uptime(void) { return (double)clock_gettime_nsec_np(CLOCK_UPTIME_RAW) / 1000000000.0; }

static float sample_at(const void *data, size_t index, uint32_t flags, uint32_t bits) {
    const unsigned char *bytes = (const unsigned char *)data + index * (bits / 8);
    if (flags & kAudioFormatFlagIsFloat) {
        if (bits == 32) { float value; memcpy(&value, bytes, sizeof(value)); return value; }
        if (bits == 64) { double value; memcpy(&value, bytes, sizeof(value)); return (float)value; }
    } else if (bits == 16) { int16_t value; memcpy(&value, bytes, sizeof(value)); return value / 32768.0f; }
    else if (bits == 32) { int32_t value; memcpy(&value, bytes, sizeof(value)); return value / 2147483648.0f; }
    return 0;
}
static void push_impl(MNPCMQueue *q, const AudioBufferList *abl, uint32_t frames,
                 uint32_t flags, uint32_t bits, double uptime) {
    if (!q || !abl || frames == 0 || !atomic_load(&q->accepting)) return;
    bool floating = (flags & kAudioFormatFlagIsFloat) != 0;
    bool valid = !(flags & kAudioFormatFlagIsBigEndian) && (flags & kAudioFormatFlagIsPacked)
        && ((floating && (bits == 32 || bits == 64)) || (!floating && (flags & kAudioFormatFlagIsSignedInteger) && (bits == 16 || bits == 32)));
    if (!valid) { atomic_store(&q->invalid_format, 1); return; }
    if (abl->mNumberBuffers > 64) { atomic_store(&q->invalid_format, 1); return; }
    uint32_t channels = 0, safe_frames = frames;
    for (uint32_t b=0; b<abl->mNumberBuffers; ++b) {
        const AudioBuffer *buffer = &abl->mBuffers[b];
        if (!buffer->mData || !buffer->mNumberChannels) continue;
        if (buffer->mNumberChannels > 64 || channels > 64 - buffer->mNumberChannels) {
            atomic_store(&q->invalid_format, 1); return;
        }
        uint64_t stride = (uint64_t)(bits / 8) * buffer->mNumberChannels;
        uint32_t available = (uint32_t)((uint64_t)buffer->mDataByteSize / stride);
        if (available < safe_frames) safe_frames = available;
        channels += buffer->mNumberChannels;
    }
    if (!channels || !safe_frames) return;
    atomic_fetch_add(&q->received, safe_frames);
    // An abnormal driver block cannot turn a callback into unbounded conversion work.
    if (safe_frames > 65536) {
        atomic_fetch_add(&q->dropped, safe_frames - 65536);
        safe_frames = 65536;
    }
    atomic_store(&q->uptime, uptime);
    double squares = 0;
    uint32_t offset = 0;
    while (offset < safe_frames) {
        uint32_t count = safe_frames-offset;
        if (count > q->capacity) count=q->capacity;
        uint64_t write = atomic_load_explicit(&q->write_index, memory_order_relaxed);
        uint64_t read = atomic_load_explicit(&q->read_index, memory_order_acquire);
        bool fits = write-read < q->slots;
        float *destination = fits ? q->samples+(write % q->slots)*q->capacity : NULL;
        for (uint32_t f=0; f<count; ++f) {
            double sum = 0;
            for (uint32_t b=0; b<abl->mNumberBuffers; ++b) {
                const AudioBuffer *buffer = &abl->mBuffers[b];
                if (!buffer->mData) continue;
                for (uint32_t c=0; c<buffer->mNumberChannels; ++c)
                    sum += sample_at(buffer->mData, ((size_t)offset+f)*buffer->mNumberChannels+c, flags, bits);
            }
            float mono = (float)(sum/channels);
            if (!isfinite(mono)) mono=0;
            squares += (double)mono*mono;
            if (destination) destination[f] = mono;
        }
        if (fits) {
            q->lengths[write % q->slots] = count;
            atomic_store_explicit(&q->write_index, write+1, memory_order_release);
        } else atomic_fetch_add(&q->dropped, count);
        offset += count;
    }
    /* The level is the loudness of about the last tenth of a second, not of
     * one callback: a microphone callback is 4096 frames, but a system-audio
     * one is around 10 ms, too short for a steady sound to read steadily. */
    q->level_squares += squares;
    q->level_frames += safe_frames;
    if (q->level_frames >= MN_LEVEL_WINDOW_FRAMES) {
        float level = (float)sqrt(q->level_squares/q->level_frames)*25.0f;
        atomic_store(&q->level, fminf(level, 1.0f));
        q->level_squares = 0;
        q->level_frames = 0;
    }
}
void mn_pcm_push(MNPCMQueue *q, const AudioBufferList *abl, uint32_t frames,
                 uint32_t flags, uint32_t bits, double uptime) {
    if(!q) return;
    atomic_fetch_add(&q->callbacks, 1);
    push_impl(q, abl, frames, flags, bits, uptime);
    atomic_fetch_sub(&q->callbacks, 1);
}
uint32_t mn_pcm_read(MNPCMQueue *q, float *destination, uint32_t capacity) {
    if (!q || !destination) return 0;
    uint64_t read = atomic_load_explicit(&q->read_index, memory_order_relaxed);
    uint64_t write = atomic_load_explicit(&q->write_index, memory_order_acquire);
    if (read == write) return 0;
    uint32_t count = q->lengths[read % q->slots];
    if (capacity < count) return 0;
    memcpy(destination, q->samples+(read % q->slots)*q->capacity, count*sizeof(float));
    atomic_store_explicit(&q->read_index, read+1, memory_order_release);
    return count;
}
void mn_pcm_mark_written(MNPCMQueue *q, uint32_t frames) { if(q) atomic_fetch_add(&q->written, frames); }
void mn_pcm_mark_discarded(MNPCMQueue *q, uint32_t frames) { if(q) atomic_fetch_add(&q->dropped, frames); }
MNPCMStats mn_pcm_stats(MNPCMQueue *q) {
    MNPCMStats s = {0}; if(!q) return s;
    s.frames_received=atomic_load(&q->received); s.frames_written=atomic_load(&q->written);
    s.dropped_frames=atomic_load(&q->dropped); s.level=atomic_load(&q->accepting) ? atomic_load(&q->level) : 0;
    s.last_buffer_uptime=atomic_load(&q->uptime); s.invalid_format=atomic_load(&q->invalid_format); return s;
}
