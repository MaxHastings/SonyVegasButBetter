// Thin ABI boundary compiled against the bundled FFmpeg 9 headers. No FFmpeg
// types leave this file. Libraries and all decoder calls belong to one worker.
extern "C" {
#include <libavformat/avformat.h>
#include <libavcodec/avcodec.h>
#include <libswscale/swscale.h>
#include <libavutil/imgutils.h>
#include <libswresample/swresample.h>
}
#include <cmath>
#include <new>
#include <vector>
#include <algorithm>
#include <cstring>
struct API {
    decltype(&avformat_open_input) open;
    decltype(&avformat_find_stream_info) info;
    decltype(&avformat_close_input) close;
    decltype(&avcodec_find_decoder) decoder;
    decltype(&avcodec_alloc_context3) alloc;
    decltype(&avcodec_parameters_to_context) params;
    decltype(&avcodec_open2) codec_open;
    decltype(&avcodec_free_context) codec_free;
    decltype(&av_packet_alloc) packet;
    decltype(&av_packet_free) packet_free;
    decltype(&av_packet_unref) packet_unref;
    decltype(&av_read_frame) read;
    decltype(&avcodec_send_packet) send;
    decltype(&avcodec_receive_frame) receive;
    decltype(&av_frame_alloc) frame;
    decltype(&av_frame_free) frame_free;
    decltype(&av_frame_unref) frame_unref;
    decltype(&av_frame_move_ref) frame_move;
    decltype(&av_seek_frame) seek;
    decltype(&avcodec_flush_buffers) flush;
    decltype(&sws_getCachedContext) scale_context;
    decltype(&sws_scale) scale;
    decltype(&sws_freeContext) scale_free;
    decltype(&swr_alloc_set_opts2) resampler;
    decltype(&swr_init) resample_init;
    decltype(&swr_convert) resample;
    decltype(&swr_get_delay) delay;
    decltype(&swr_free) resample_free;
    decltype(&av_channel_layout_default) layout;

};
static_assert(sizeof(API) == 29 * sizeof(void*), "FFmpeg API table layout changed");
struct Decoder {
    API a;
    AVFormatContext* f = nullptr;
    AVCodecContext* c = nullptr;
    AVPacket* packet = nullptr;
    AVFrame* current = nullptr;
    AVFrame* next = nullptr;
    SwsContext* sws = nullptr;
    SwrContext* swr = nullptr;
    std::vector<float> audio;
    int64_t audio_start = 0;
    int64_t audio_requested = -1;
    bool audio_valid = false, audio_drained = false;

    int stream = 0;
    double origin = 0, current_pts = -INFINITY, next_pts = -INFINITY;
    bool draining = false, eof = false;
    int (*cancel)(void*) = nullptr;
    void* opaque = nullptr;
};
static int interrupt(void* ptr) {
    auto d = static_cast<Decoder*>(ptr);
    return d->cancel && d->cancel(d->opaque);
}
extern "C" void svbb_video_close(void* ptr) {
    auto d = static_cast<Decoder*>(ptr);
    if (!d) return;
    if (d->sws) d->a.scale_free(d->sws);
    if (d->swr) d->a.resample_free(&d->swr);
    d->a.frame_free(&d->current); d->a.frame_free(&d->next);
    d->a.packet_free(&d->packet); d->a.codec_free(&d->c);
    if (d->f) d->a.close(&d->f);
    delete d;
}
extern "C" void* svbb_video_open(void** api, const char* path, int stream, double origin) {
    auto d = new (std::nothrow) Decoder;
    if (!d) return nullptr;
    std::memcpy(&d->a, api, sizeof(API));
    d->stream = stream; d->origin = origin;
    if (d->a.open(&d->f, path, nullptr, nullptr) < 0 || d->a.info(d->f, nullptr) < 0
        || stream < 0 || (unsigned)stream >= d->f->nb_streams) { svbb_video_close(d); return nullptr; }
    d->f->interrupt_callback = {interrupt, d};
    auto codec = d->a.decoder(d->f->streams[stream]->codecpar->codec_id);
    if (!codec) { svbb_video_close(d); return nullptr; }
    d->c = d->a.alloc(codec);
    if (!d->c || d->a.params(d->c, d->f->streams[stream]->codecpar) < 0) { svbb_video_close(d); return nullptr; }
    d->c->thread_count = 2;
    d->c->pkt_timebase = d->f->streams[stream]->time_base;
    if (d->a.codec_open(d->c, codec, nullptr) < 0) { svbb_video_close(d); return nullptr; }
    d->packet = d->a.packet(); d->current = d->a.frame(); d->next = d->a.frame();
    if (!d->packet || !d->current || !d->next) { svbb_video_close(d); return nullptr; }
    return d;
}
static int next_frame(Decoder* d) {
    while (!interrupt(d)) {
        int r = d->a.receive(d->c, d->next);
        if (r == 0) return 1;
        if (r == AVERROR_EOF) return 0;
        if (r != AVERROR(EAGAIN)) return r;
        if (d->draining) return 0;
        do {
            r = d->a.read(d->f, d->packet);
            if (r < 0) {
                if (r != AVERROR_EOF) return r;
                d->a.send(d->c, nullptr); d->draining = true; break;
            }
            if (d->packet->stream_index == d->stream) {
                r = d->a.send(d->c, d->packet);
                d->a.packet_unref(d->packet);
                if (r < 0) return r;
                break;
            }
            d->a.packet_unref(d->packet);
        } while (!interrupt(d));
    }
    return AVERROR_EXIT;
}
// Retains the frame covering t plus one lookahead frame. Non-sequential requests
// seek backwards to a keyframe, then decode presentation timestamps up to t.
extern "C" int svbb_video_frame(void* ptr, double t, unsigned char* pixels, int width, int height,
                                void* opaque, int (*cancel)(void*), double* actual) {
    auto d = static_cast<Decoder*>(ptr);
    struct ClearCallback { Decoder* d; ~ClearCallback() { d->cancel = nullptr; d->opaque = nullptr; } } clear{d};
    d->opaque = opaque; d->cancel = cancel;
    auto tb = d->f->streams[d->stream]->time_base;
    double unit = (double)tb.num / tb.den;
    if (!d->current->data[0] || t + 0.0000001 < d->current_pts ||
        (t > d->current_pts + 0.75 && (!d->next->data[0] || t >= d->next_pts))) {
        int64_t stamp = (int64_t)std::floor((t + d->origin) / unit);
        int r = d->a.seek(d->f, d->stream, stamp, AVSEEK_FLAG_BACKWARD);
        if (r < 0) return r;
        d->a.flush(d->c); d->a.packet_unref(d->packet);
        d->a.frame_unref(d->current); d->a.frame_unref(d->next);
        d->current_pts = d->next_pts = -INFINITY; d->eof = d->draining = false;
    }
    while (!interrupt(d)) {
        if (!d->next->data[0] && !d->eof) {
            int r = next_frame(d);
            if (r < 0) return r;
            d->eof = r == 0;
            if (!d->eof) {
                auto pts = d->next->best_effort_timestamp;
                if (pts == AV_NOPTS_VALUE) return AVERROR_INVALIDDATA;
                d->next_pts = pts * unit - d->origin;
            }
        }
        if (d->eof || (d->current->data[0] && d->next_pts > t + 0.0000001)) break;
        d->a.frame_unref(d->current); d->a.frame_move(d->current, d->next);
        d->current_pts = d->next_pts;
    }
    if (interrupt(d)) return AVERROR_EXIT;
    if (!d->current->data[0]) return AVERROR_INVALIDDATA;
    // Aspect-preserving fit with opaque black letterboxing.
    int sw = d->current->width, sh = d->current->height;
    AVRational sar = d->current->sample_aspect_ratio;
    double ratio = (double)sw / sh * (sar.num > 0 && sar.den > 0 ? (double)sar.num / sar.den : 1.0);
    int w = width, h = (int)std::round(width / ratio);
    if (h > height) { h = height; w = (int)std::round(height * ratio); }
    if (w < 1) w = 1; if (h < 1) h = 1;
    for (int n = 0; n < width * height; n++) { pixels[n*4] = pixels[n*4+1] = pixels[n*4+2] = 0; pixels[n*4+3] = 255; }
    d->sws = d->a.scale_context(d->sws, sw, sh, (AVPixelFormat)d->current->format, w, h, AV_PIX_FMT_RGBA, SWS_BILINEAR, nullptr, nullptr, nullptr);
    if (!d->sws) return AVERROR(ENOMEM);
    unsigned char* out[4] = {pixels + ((height-h)/2 * width + (width-w)/2)*4, nullptr, nullptr, nullptr};
    int stride[4] = {width*4, 0, 0, 0};
    int r = d->a.scale(d->sws, d->current->data, d->current->linesize, 0, sh, out, stride);
    *actual = d->current_pts;
    d->cancel = nullptr; d->opaque = nullptr;
    return r < 0 ? r : 0;
}

// Bounded, seekable stereo float decoder. The caller runs on the demux/cache
// worker, never on the device audio callback. Source PTS include original gaps.
extern "C" int svbb_audio_read(void* ptr, int64_t start, int count, float* out) {
    auto d = static_cast<Decoder*>(ptr);
    std::fill(out,out+count*2,0.f);
    auto tb = d->f->streams[d->stream]->time_base;
    double unit = (double)tb.num/tb.den;
    int64_t block_end = d->audio_start + (int64_t)d->audio.size()/2;
    if (!d->audio_valid || (start < d->audio_start && start < d->audio_requested) || start > block_end + 48000) {
        // Preroll settles codec and resampling state without decoding the file head.
        int64_t timestamp=(int64_t)std::floor((std::max(0.0,start/48000.0-1.0)+d->origin)/unit);
        int r=d->a.seek(d->f,d->stream,timestamp,AVSEEK_FLAG_BACKWARD);
        if (r<0) return r;
        d->a.flush(d->c);d->a.packet_unref(d->packet);d->a.frame_unref(d->next);
        d->a.resample_free(&d->swr); d->draining=d->eof=d->audio_drained=false;
        d->audio.clear(); d->audio_valid=true; d->audio_start=start;
    }
    d->audio_requested = start;
    int64_t cursor=start, end=start+count;
    while (cursor<end) {
        block_end=d->audio_start+(int64_t)d->audio.size()/2;
        if (cursor>=d->audio_start && cursor<block_end) {
            int n=(int)std::min(end-cursor,block_end-cursor);
            std::copy_n(d->audio.data()+(cursor-d->audio_start)*2,n*2,out+(cursor-start)*2);
            cursor+=n;continue;
        }
        if (cursor<d->audio_start && !d->audio.empty()) {
            cursor=std::min(end,d->audio_start);continue;
        }
        if (d->eof) break;
        d->a.frame_unref(d->next);
        int r=next_frame(d);
        if (r<0) return r;
        if (!r) {
            if (d->swr && !d->audio_drained) {
                d->audio_drained=true;d->audio_start=block_end;d->audio.resize(512*2);
                uint8_t* data=(uint8_t*)d->audio.data();
                int n=d->a.resample(d->swr,&data,512,nullptr,0);
                if (n<0)return n;d->audio.resize(n*2);if(n)continue;
            }
            d->eof=true;break;
        }
        auto frame=d->next;
        if (!d->swr) {
            AVChannelLayout layout{};d->a.layout(&layout,2);
            r=d->a.resampler(&d->swr,&layout,AV_SAMPLE_FMT_FLT,48000,&frame->ch_layout,(AVSampleFormat)frame->format,frame->sample_rate,0,nullptr);
            if(r<0)return r;
            r=d->a.resample_init(d->swr);if(r<0)return r;
        }
        if(frame->best_effort_timestamp==AV_NOPTS_VALUE)return AVERROR_INVALIDDATA;
        int64_t delay=d->a.delay(d->swr,frame->sample_rate);
        d->audio_start=(int64_t)std::llround((frame->best_effort_timestamp*unit-d->origin)*48000.0-delay*48000.0/frame->sample_rate);
        int capacity=(int)((frame->nb_samples+delay)*48000LL/frame->sample_rate+256);
        d->audio.resize(capacity*2);
        uint8_t* data=(uint8_t*)d->audio.data();
        int n=d->a.resample(d->swr,&data,capacity,(const uint8_t**)frame->extended_data,frame->nb_samples);
        if(n<0)return n;d->audio.resize(n*2);
    }
    return 0;
}
