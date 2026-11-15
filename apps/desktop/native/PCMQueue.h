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

#ifndef MEETING_PCM_QUEUE_H
#define MEETING_PCM_QUEUE_H
#include <AudioToolbox/AudioToolbox.h>
#include <stdint.h>
#include <stddef.h>

typedef struct MNPCMQueue MNPCMQueue;
typedef struct {
    uint64_t frames_received, frames_written, dropped_frames;
    float level;
    double last_buffer_uptime;
    uint32_t invalid_format;
} MNPCMStats;
MNPCMQueue *mn_pcm_create(uint32_t slots, uint32_t frames_per_slot);
void mn_pcm_destroy(MNPCMQueue *queue);
void mn_pcm_close(MNPCMQueue *queue);
uint32_t mn_pcm_callbacks_active(MNPCMQueue *queue);
double mn_pcm_uptime(void);
void mn_pcm_push(MNPCMQueue *queue, const AudioBufferList *buffers, uint32_t frames,
                 uint32_t flags, uint32_t bits, double uptime);
uint32_t mn_pcm_read(MNPCMQueue *queue, float *destination, uint32_t capacity);
void mn_pcm_mark_written(MNPCMQueue *queue, uint32_t frames);
void mn_pcm_mark_discarded(MNPCMQueue *queue, uint32_t frames);
MNPCMStats mn_pcm_stats(MNPCMQueue *queue);
#endif
