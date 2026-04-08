#include "theorafile.h"
#include <stdlib.h>

typedef struct TFHandle {
    OggTheora_File file;
} TFHandle;

TFHandle* tfh_open_callbacks(void *datasource, tf_callbacks io)
{
    TFHandle* h = (TFHandle*) malloc(sizeof(TFHandle));
    if (!h) return NULL;
    if (tf_open_callbacks(datasource, &h->file, io) < 0) {
        free(h);
        return NULL;
    }
    return h;
}

void tfh_close(TFHandle* h)
{
    if (!h) return;
    tf_close(&h->file);
    free(h);
}

int tfh_hasvideo(TFHandle* h)
{
    if (!h) return 0;
    return tf_hasvideo(&h->file);
}

int tfh_hasaudio(TFHandle* h)
{
    if (!h) return 0;
    return tf_hasaudio(&h->file);
}

void tfh_videoinfo(TFHandle* h, int *width, int *height, double *fps, int *fmt)
{
    if (!h) return;
    th_pixel_fmt f = 0;
    tf_videoinfo(&h->file, width, height, fps, &f);
    if (fmt) *fmt = (int) f;
}

void tfh_audioinfo(TFHandle* h, int *channels, int *samplerate)
{
    if (!h) return;
    tf_audioinfo(&h->file, channels, samplerate);
}

int tfh_eos(TFHandle* h)
{
    if (!h) return 1;
    return tf_eos(&h->file);
}

void tfh_reset(TFHandle* h)
{
    if (!h) return;
    tf_reset(&h->file);
}

int tfh_readvideo(TFHandle* h, char *buffer, int numframes)
{
    if (!h) return -1;
    return tf_readvideo(&h->file, buffer, numframes);
}

int tfh_readaudio(TFHandle* h, float *buffer, int samples)
{
    if (!h) return -1;
    return tf_readaudio(&h->file, buffer, samples);
}
