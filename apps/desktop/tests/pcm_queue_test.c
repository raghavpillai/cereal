/* Original queue tests. No capture device, microphone permission or user audio
 * is accessed. PCMQueue retains its separate OpenOats MIT attribution. */
#include "PCMQueue.h"
#include <float.h>
#include <math.h>
#include <pthread.h>
#include <sched.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static unsigned checks, failures, cases;
#define CHECK(expression) do { \
    ++checks; \
    if (!(expression)) { \
        ++failures; \
        fprintf(stderr, "%s:%d: %s\n", __FILE__, __LINE__, #expression); \
    } \
} while (0)
#define REQUIRE(expression) do { \
    if (!(expression)) { fprintf(stderr, "Required allocation failed: %s\n", #expression); exit(2); } \
} while (0)

static const uint32_t float_flags = kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked;
static const uint32_t integer_flags = kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsPacked;

static AudioBufferList *buffers(uint32_t count) {
    size_t bytes = offsetof(AudioBufferList, mBuffers) + (size_t)count * sizeof(AudioBuffer);
    if (bytes < sizeof(AudioBufferList)) bytes = sizeof(AudioBufferList);
    AudioBufferList *list = calloc(1, bytes);
    REQUIRE(list);
    list->mNumberBuffers = count;
    return list;
}

static MNPCMQueue *queue(uint32_t slots, uint32_t capacity) {
    MNPCMQueue *result = mn_pcm_create(slots, capacity);
    REQUIRE(result);
    return result;
}

static void close_to(float actual, float expected) {
    CHECK(isfinite(actual));
    CHECK(fabsf(actual - expected) < 0.000001f);
}

static void samples_equal(const float *actual, const float *expected, uint32_t count) {
    for (uint32_t i = 0; i < count; ++i) close_to(actual[i], expected[i]);
}

static void mono_push(MNPCMQueue *q, float *samples, uint32_t count, double uptime) {
    AudioBufferList list = { .mNumberBuffers = 1, .mBuffers = {{
        .mNumberChannels = 1, .mDataByteSize = count * sizeof(float), .mData = samples
    }} };
    mn_pcm_push(q, &list, count, float_flags, 32, uptime);
}

static void limits_and_nulls(void) {
    ++cases;
    _Atomic uint64_t counter = 0;
    _Atomic uint32_t active = 0;
    _Atomic float level = 0;
    _Atomic double uptime = 0;
    _Atomic bool accepting = false;
    CHECK(atomic_is_lock_free(&counter) && atomic_is_lock_free(&active));
    CHECK(atomic_is_lock_free(&level) && atomic_is_lock_free(&uptime) && atomic_is_lock_free(&accepting));
    const double first_tick = mn_pcm_uptime(), second_tick = mn_pcm_uptime();
    CHECK(isfinite(first_tick) && first_tick > 0 && second_tick >= first_tick);
    CHECK(mn_pcm_create(0, 4) == NULL);
    CHECK(mn_pcm_create(1, 4) == NULL);
    CHECK(mn_pcm_create(129, 4) == NULL);
    CHECK(mn_pcm_create(2, 0) == NULL);
    CHECK(mn_pcm_create(2, 65537) == NULL);
    mn_pcm_destroy(NULL);
    mn_pcm_close(NULL);
    mn_pcm_push(NULL, NULL, 10, float_flags, 32, 1);
    mn_pcm_mark_written(NULL, 1);
    mn_pcm_mark_discarded(NULL, 1);
    CHECK(mn_pcm_read(NULL, NULL, 0) == 0);
    CHECK(mn_pcm_callbacks_active(NULL) == 0);
    MNPCMStats empty = mn_pcm_stats(NULL);
    CHECK(empty.frames_received == 0 && empty.frames_written == 0 && empty.dropped_frames == 0);
    MNPCMQueue *q = queue(2, 1);
    MNPCMStats stats = mn_pcm_stats(q);
    CHECK(stats.frames_received == 0 && stats.frames_written == 0 && stats.dropped_frames == 0);
    CHECK(stats.invalid_format == 0 && stats.level == 0 && stats.last_buffer_uptime == 0);
    CHECK(mn_pcm_callbacks_active(q) == 0);
    mn_pcm_destroy(q);
}

static void float_interleaved_downmix(void) {
    ++cases;
    MNPCMQueue *q = queue(2, 8);
    float input[] = {0.25f, 0.75f, -1, 1, 0.125f, 0.375f, 0, -0.5f};
    float expected[] = {0.5f, 0, 0.25f, -0.25f}, output[8] = {0};
    AudioBufferList list = { .mNumberBuffers = 1, .mBuffers = {{
        .mNumberChannels = 2, .mDataByteSize = sizeof(input), .mData = input
    }} };
    mn_pcm_push(q, &list, 4, float_flags, 32, 12.5);
    CHECK(mn_pcm_read(q, output, 8) == 4);
    samples_equal(output, expected, 4);
    MNPCMStats stats = mn_pcm_stats(q);
    CHECK(stats.frames_received == 4 && stats.frames_written == 0 && stats.dropped_frames == 0);
    CHECK(stats.last_buffer_uptime == 12.5 && stats.invalid_format == 0);
    close_to(stats.level, 1);
    mn_pcm_destroy(q);
}

static void planar_and_mixed_channel_buffers(void) {
    ++cases;
    MNPCMQueue *q = queue(2, 8);
    float left[] = {1, -0.5f, 0.25f}, right[] = {-1, 0.5f, 0.75f};
    float expected[] = {0, 0, 0.5f}, output[8] = {0};
    AudioBufferList *list = buffers(2);
    list->mBuffers[0] = (AudioBuffer){1, sizeof(left), left};
    list->mBuffers[1] = (AudioBuffer){1, sizeof(right), right};
    mn_pcm_push(q, list, 3, float_flags | kAudioFormatFlagIsNonInterleaved, 32, 1);
    CHECK(mn_pcm_read(q, output, 8) == 3);
    samples_equal(output, expected, 3);

    // A mono buffer and a stereo buffer contain three channels, not two.
    float stereo[] = {0.25f, 0.5f, 0, 1, -0.25f, 0.75f};
    float mono[] = {0.75f, -1, 1};
    float thirds[] = {0.5f, 0, 0.5f};
    list->mBuffers[0] = (AudioBuffer){1, sizeof(mono), mono};
    list->mBuffers[1] = (AudioBuffer){2, sizeof(stereo), stereo};
    mn_pcm_push(q, list, 3, float_flags, 32, 2);
    CHECK(mn_pcm_read(q, output, 8) == 3);
    samples_equal(output, thirds, 3);
    free(list);
    mn_pcm_destroy(q);
}

static void signed_integer_scaling(void) {
    ++cases;
    MNPCMQueue *q = queue(2, 8);
    int16_t pcm16[] = {INT16_MIN, -16384, 0, 16384, INT16_MAX};
    float expected16[] = {-1, -0.5f, 0, 0.5f, 32767.0f / 32768.0f}, output[8] = {0};
    AudioBufferList list = { .mNumberBuffers = 1, .mBuffers = {{
        .mNumberChannels = 1, .mDataByteSize = sizeof(pcm16), .mData = pcm16
    }} };
    mn_pcm_push(q, &list, 5, integer_flags, 16, 1);
    CHECK(mn_pcm_read(q, output, 8) == 5);
    samples_equal(output, expected16, 5);
    int32_t pcm32[] = {INT32_MIN, 0, INT32_MAX, INT32_MAX, INT32_MIN, INT32_MAX};
    float expected32[] = {-0.5f, 1, 0};
    list.mBuffers[0] = (AudioBuffer){2, sizeof(pcm32), pcm32};
    mn_pcm_push(q, &list, 3, integer_flags, 32, 2);
    CHECK(mn_pcm_read(q, output, 8) == 3);
    samples_equal(output, expected32, 3);
    mn_pcm_destroy(q);
}

static void nonfinite_float_and_silence(void) {
    ++cases;
    MNPCMQueue *q = queue(2, 8);
    double input64[] = {0.75, -0.125, NAN, INFINITY, -INFINITY, DBL_MAX, -0.5};
    float expected64[] = {0.75f, -0.125f, 0, 0, 0, 0, -0.5f}, output[8] = {0};
    AudioBufferList list = { .mNumberBuffers = 1, .mBuffers = {{
        .mNumberChannels = 1, .mDataByteSize = sizeof(input64), .mData = input64
    }} };
    mn_pcm_push(q, &list, 7, float_flags, 64, 1);
    CHECK(mn_pcm_read(q, output, 8) == 7);
    samples_equal(output, expected64, 7);
    float input32[] = {NAN, INFINITY, -INFINITY, 0};
    float zeros[] = {0, 0, 0, 0};
    mono_push(q, input32, 4, 2);
    CHECK(mn_pcm_read(q, output, 8) == 4);
    samples_equal(output, zeros, 4);
    close_to(mn_pcm_stats(q).level, 0);
    mono_push(q, zeros, 4, 3);
    CHECK(mn_pcm_read(q, output, 8) == 4);
    samples_equal(output, zeros, 4);
    MNPCMStats stats = mn_pcm_stats(q);
    CHECK(stats.frames_received == 15 && stats.invalid_format == 0);
    close_to(stats.level, 0);
    float quiet[] = {0.01f, -0.01f};
    mono_push(q, quiet, 2, 4);
    close_to(mn_pcm_stats(q).level, 0.25f);
    mn_pcm_destroy(q);
}

static void short_and_empty_buffers(void) {
    ++cases;
    MNPCMQueue *q = queue(2, 8);
    float incomplete_stereo[] = {0.25f, 0.75f, 100};
    float output[8] = {0};
    AudioBufferList list = { .mNumberBuffers = 1, .mBuffers = {{
        .mNumberChannels = 2, .mDataByteSize = sizeof(incomplete_stereo), .mData = incomplete_stereo
    }} };
    mn_pcm_push(q, &list, 1024, float_flags, 32, 1);
    CHECK(mn_pcm_read(q, output, 8) == 1);
    close_to(output[0], 0.5f);
    float long_channel[] = {1, 0.5f, 100, 100};
    float short_channel[] = {-1, 0.5f};
    AudioBufferList *planar = buffers(3);
    planar->mBuffers[0] = (AudioBuffer){1, sizeof(long_channel), long_channel};
    planar->mBuffers[1] = (AudioBuffer){1, sizeof(short_channel), short_channel};
    planar->mBuffers[2] = (AudioBuffer){1, UINT32_MAX, NULL};
    mn_pcm_push(q, planar, 4, float_flags, 32, 2);
    CHECK(mn_pcm_read(q, output, 8) == 2);
    close_to(output[0], 0);
    close_to(output[1], 0.5f);
    // A byte-short sample contributes no frame and must never be dereferenced.
    list.mBuffers[0] = (AudioBuffer){1, sizeof(float) - 1, incomplete_stereo};
    mn_pcm_push(q, &list, 1, float_flags, 32, 3);
    list.mBuffers[0].mNumberChannels = 0;
    mn_pcm_push(q, &list, 1, float_flags, 32, 3);
    list.mNumberBuffers = 0;
    mn_pcm_push(q, &list, 1, float_flags, 32, 3);
    mn_pcm_push(q, NULL, 1, float_flags, 32, 3);
    mn_pcm_push(q, planar, 0, float_flags, 32, 3);
    CHECK(mn_pcm_read(q, output, 8) == 0);
    CHECK(mn_pcm_stats(q).frames_received == 3);
    CHECK(mn_pcm_stats(q).last_buffer_uptime == 2);
    free(planar);
    mn_pcm_destroy(q);
}

static void bounded_overflow_and_accounting(void) {
    ++cases;
    MNPCMQueue *q = queue(2, 2);
    float input[] = {0.1f, 0.2f, 0.3f, 0.4f, 0.5f, 0.6f, 0.7f};
    float output[4] = {9, 9, 9, 9};
    mono_push(q, input, 7, 10);
    MNPCMStats stats = mn_pcm_stats(q);
    CHECK(stats.frames_received == 7 && stats.dropped_frames == 3 && stats.frames_written == 0);
    // A destination smaller than the queued slot must not consume or overwrite it.
    CHECK(mn_pcm_read(q, output, 1) == 0);
    CHECK(output[0] == 9 && output[1] == 9);
    CHECK(mn_pcm_read(q, output, 4) == 2);
    samples_equal(output, input, 2);
    mn_pcm_mark_written(q, 2);
    CHECK(mn_pcm_read(q, output, 4) == 2);
    samples_equal(output, input + 2, 2);
    mn_pcm_mark_discarded(q, 2); // A failed file write still reconciles counters.
    CHECK(mn_pcm_read(q, output, 4) == 0);
    stats = mn_pcm_stats(q);
    CHECK(stats.frames_received == stats.frames_written + stats.dropped_frames);
    CHECK(stats.frames_written == 2 && stats.dropped_frames == 5);
    mono_push(q, input + 6, 1, 11);
    CHECK(mn_pcm_read(q, output, 4) == 1);
    close_to(output[0], 0.7f);
    mn_pcm_destroy(q);
}

static void ring_wrap_preserves_order(void) {
    ++cases;
    MNPCMQueue *q = queue(3, 2);
    float output[2];
    for (unsigned i = 0; i < 100; ++i) {
        float pair[] = {(float)i, -(float)i};
        mono_push(q, pair, 2, i);
        CHECK(mn_pcm_read(q, output, 2) == 2);
        samples_equal(output, pair, 2);
        mn_pcm_mark_written(q, 2);
    }
    MNPCMStats stats = mn_pcm_stats(q);
    CHECK(stats.frames_received == 200 && stats.frames_written == 200 && stats.dropped_frames == 0);
    mn_pcm_destroy(q);
}

static void close_drains_and_rejects_new_frames(void) {
    ++cases;
    MNPCMQueue *q = queue(2, 4);
    float input[] = {1, 2, 3, 4, 5, 6}, output[4] = {0};
    mono_push(q, input, 6, 1);
    mn_pcm_close(q);
    mn_pcm_close(q);
    mono_push(q, input, 6, 99);
    CHECK(mn_pcm_stats(q).level == 0);
    CHECK(mn_pcm_stats(q).frames_received == 6 && mn_pcm_stats(q).last_buffer_uptime == 1);
    CHECK(mn_pcm_read(q, output, 4) == 4);
    samples_equal(output, input, 4);
    mn_pcm_mark_written(q, 4);
    CHECK(mn_pcm_read(q, output, 4) == 2);
    samples_equal(output, input + 4, 2);
    mn_pcm_mark_written(q, 2);
    CHECK(mn_pcm_read(q, output, 4) == 0);
    CHECK(mn_pcm_stats(q).frames_written == 6);
    CHECK(mn_pcm_callbacks_active(q) == 0);
    mn_pcm_destroy(q);
}

static void invalid_formats_do_not_mutate_queued_audio(void) {
    ++cases;
    const struct { uint32_t flags, bits; } invalid[] = {
        {kAudioFormatFlagIsFloat, 32}, // Unpacked/aligned PCM is unsupported.
        {float_flags | kAudioFormatFlagIsBigEndian, 32},
        {float_flags, 16}, {integer_flags, 24},
        {kAudioFormatFlagIsPacked, 16}, {integer_flags, 0},
    };
    float input[] = {0.25f, -0.5f}, output[2] = {0};
    for (unsigned i = 0; i < sizeof(invalid) / sizeof(invalid[0]); ++i) {
        MNPCMQueue *q = queue(2, 4);
        mono_push(q, input, 2, 5);
        AudioBufferList list = { .mNumberBuffers = 1, .mBuffers = {{1, sizeof(input), input}} };
        mn_pcm_push(q, &list, 2, invalid[i].flags, invalid[i].bits, 100);
        MNPCMStats stats = mn_pcm_stats(q);
        CHECK(stats.invalid_format == 1 && stats.frames_received == 2 && stats.last_buffer_uptime == 5);
        CHECK(mn_pcm_read(q, output, 2) == 2);
        samples_equal(output, input, 2);
        CHECK(mn_pcm_read(q, output, 2) == 0);
        mn_pcm_destroy(q);
    }
}

static void malformed_channel_count_cannot_overflow_frame_size(void) {
    ++cases;
    MNPCMQueue *q = queue(2, 4);
    float value = 0.25f, output[4];
    AudioBufferList list = { .mNumberBuffers = 1, .mBuffers = {{
        .mNumberChannels = UINT32_C(1) << 30,
        .mDataByteSize = sizeof(value), .mData = &value
    }} };
    // Metadata claiming more channels than fit in the buffer must reject the
    // frame, rather than wrapping bytes-per-frame to zero in a 32-bit product.
    mn_pcm_push(q, &list, 1, float_flags, 32, 1);
    CHECK(mn_pcm_read(q, output, 4) == 0);
    CHECK(mn_pcm_stats(q).frames_received == 0);
    mn_pcm_destroy(q);
}

static void packed_samples_need_no_pointer_alignment_assumption(void) {
    ++cases;
    MNPCMQueue *q = queue(2, 4);
    _Alignas(8) unsigned char storage[32];
    const float values32[] = {0.25f, 0.75f};
    const double values64[] = {0.5, -0.25};
    const int16_t values16[] = {INT16_MIN, 16384};
    const int32_t integers32[] = {INT32_MIN, 0};
    const struct {
        const void *data; size_t bytes; uint32_t flags, bits; float expected;
    } inputs[] = {
        {values32, sizeof(values32), float_flags, 32, 0.5f},
        {values64, sizeof(values64), float_flags, 64, 0.125f},
        {values16, sizeof(values16), integer_flags, 16, -0.25f},
        {integers32, sizeof(integers32), integer_flags, 32, -0.5f},
    };
    for (unsigned i = 0; i < sizeof(inputs) / sizeof(inputs[0]); ++i) {
        memcpy(storage + 1, inputs[i].data, inputs[i].bytes);
        AudioBufferList list = { .mNumberBuffers = 1, .mBuffers = {{
            .mNumberChannels = 2, .mDataByteSize = (uint32_t)inputs[i].bytes, .mData = storage + 1
        }} };
        float output[4] = {0};
        mn_pcm_push(q, &list, 1, inputs[i].flags, inputs[i].bits, 1);
        CHECK(mn_pcm_read(q, output, 4) == 1);
        close_to(output[0], inputs[i].expected);
    }
    mn_pcm_destroy(q);
}

static void oversized_callbacks_are_bounded_and_accounted(void) {
    ++cases;
    MNPCMQueue *q = queue(2, 32768);
    float *input = calloc(65537, sizeof(float));
    float *output = calloc(32768, sizeof(float));
    REQUIRE(input && output);
    input[65536] = 1; // Beyond the conversion budget: must not affect the meter.
    mono_push(q, input, 65537, 1);
    MNPCMStats stats = mn_pcm_stats(q);
    CHECK(stats.frames_received == 65537 && stats.dropped_frames == 1);
    close_to(stats.level, 0);
    for (unsigned slot = 0; slot < 2; ++slot) {
        CHECK(mn_pcm_read(q, output, 32768) == 32768);
        close_to(output[0], 0);
        close_to(output[32767], 0);
        mn_pcm_mark_written(q, 32768);
    }
    CHECK(mn_pcm_read(q, output, 32768) == 0);
    stats = mn_pcm_stats(q);
    CHECK(stats.frames_written == 65536);
    CHECK(stats.frames_written + stats.dropped_frames == stats.frames_received);
    free(input);
    free(output);
    mn_pcm_destroy(q);
}

static void excessive_buffer_count_is_rejected_before_access(void) {
    ++cases;
    MNPCMQueue *q = queue(2, 4);
    AudioBufferList list = {.mNumberBuffers = 65};
    // Deliberately only one physical AudioBuffer: the metadata limit must be
    // checked before traversing the oversized list, not after dereferencing it.
    mn_pcm_push(q, &list, 1, float_flags, 32, 1);
    MNPCMStats stats = mn_pcm_stats(q);
    CHECK(stats.invalid_format == 1 && stats.frames_received == 0);
    mn_pcm_destroy(q);
}

typedef struct {
    MNPCMQueue *queue;
    _Atomic bool start, done;
    unsigned consumed, order_errors;
} Concurrent;

static void *producer(void *context) {
    Concurrent *test = context;
    while (!atomic_load(&test->start)) sched_yield();
    for (unsigned i = 1; i <= 50000; ++i) {
        float value = (float)i;
        mono_push(test->queue, &value, 1, i);
    }
    mn_pcm_close(test->queue);
    atomic_store_explicit(&test->done, true, memory_order_release);
    return NULL;
}

static void *consumer(void *context) {
    Concurrent *test = context;
    float value, previous = 0;
    for (;;) {
        if (mn_pcm_read(test->queue, &value, 1)) {
            if (!(value > previous && value <= 50000 && floorf(value) == value)) ++test->order_errors;
            previous = value;
            ++test->consumed;
            mn_pcm_mark_written(test->queue, 1);
        } else if (atomic_load_explicit(&test->done, memory_order_acquire)) {
            // One more read after observing completion closes the empty/done race.
            if (!mn_pcm_read(test->queue, &value, 1)) break;
            if (!(value > previous && value <= 50000 && floorf(value) == value)) ++test->order_errors;
            previous = value;
            ++test->consumed;
            mn_pcm_mark_written(test->queue, 1);
        } else sched_yield();
    }
    return NULL;
}

static void concurrent_single_producer_consumer(void) {
    ++cases;
    Concurrent test = {.queue = queue(8, 1)};
    pthread_t writer, reader;
    REQUIRE(pthread_create(&writer, NULL, producer, &test) == 0);
    REQUIRE(pthread_create(&reader, NULL, consumer, &test) == 0);
    atomic_store(&test.start, true);
    REQUIRE(pthread_join(writer, NULL) == 0);
    REQUIRE(pthread_join(reader, NULL) == 0);
    MNPCMStats stats = mn_pcm_stats(test.queue);
    CHECK(test.order_errors == 0 && test.consumed > 0);
    CHECK(stats.frames_received == 50000);
    CHECK(stats.frames_written == test.consumed);
    CHECK(stats.frames_written + stats.dropped_frames == stats.frames_received);
    CHECK(stats.level == 0 && mn_pcm_callbacks_active(test.queue) == 0);
    mn_pcm_destroy(test.queue);
}

int main(void) {
    limits_and_nulls();
    float_interleaved_downmix();
    planar_and_mixed_channel_buffers();
    signed_integer_scaling();
    nonfinite_float_and_silence();
    short_and_empty_buffers();
    bounded_overflow_and_accounting();
    ring_wrap_preserves_order();
    close_drains_and_rejects_new_frames();
    invalid_formats_do_not_mutate_queued_audio();
    packed_samples_need_no_pointer_alignment_assumption();
    malformed_channel_count_cannot_overflow_frame_size();
    oversized_callbacks_are_bounded_and_accounted();
    excessive_buffer_count_is_rejected_before_access();
    concurrent_single_producer_consumer();
    printf("PCM queue: %u cases, %u checks, %u failures\n", cases, checks, failures);
    return failures ? EXIT_FAILURE : EXIT_SUCCESS;
}
