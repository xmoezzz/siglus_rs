use std::os::raw::{c_double, c_int, c_void};

use anyhow::{anyhow, Result};

#[repr(C)]
pub struct tf_callbacks {
    pub read_func: Option<unsafe extern "C" fn(*mut c_void, usize, usize, *mut c_void) -> usize>,
    pub seek_func: Option<unsafe extern "C" fn(*mut c_void, i64, c_int) -> c_int>,
    pub close_func: Option<unsafe extern "C" fn(*mut c_void) -> c_int>,
}

#[repr(C)]
pub struct TfHandle {
    _private: [u8; 0],
}

extern "C" {
    fn tfh_open_callbacks(datasource: *mut c_void, io: tf_callbacks) -> *mut TfHandle;
    fn tfh_close(h: *mut TfHandle);
    fn tfh_hasvideo(h: *mut TfHandle) -> c_int;
    fn tfh_hasaudio(h: *mut TfHandle) -> c_int;
    fn tfh_videoinfo(
        h: *mut TfHandle,
        width: *mut c_int,
        height: *mut c_int,
        fps: *mut c_double,
        fmt: *mut c_int,
    );
    fn tfh_audioinfo(h: *mut TfHandle, channels: *mut c_int, samplerate: *mut c_int);
    fn tfh_eos(h: *mut TfHandle) -> c_int;
    fn tfh_reset(h: *mut TfHandle);
    fn tfh_readvideo(h: *mut TfHandle, buffer: *mut i8, numframes: c_int) -> c_int;
    fn tfh_readaudio(h: *mut TfHandle, buffer: *mut f32, samples: c_int) -> c_int;
}

pub const TH_PF_420: i32 = 0;
pub const TH_PF_422: i32 = 2;
pub const TH_PF_444: i32 = 3;

pub struct DataSource {
    data: Vec<u8>,
    pos: usize,
}

impl DataSource {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data, pos: 0 }
    }

    fn seek(&mut self, offset: i64, origin: c_int) -> c_int {
        let new_pos = match origin {
            0 => offset as i64,
            1 => self.pos as i64 + offset,
            2 => self.data.len() as i64 + offset,
            _ => return -1,
        };
        let clamped = new_pos.clamp(0, self.data.len() as i64) as usize;
        self.pos = clamped;
        0
    }

    fn read(&mut self, ptr: *mut c_void, size: usize, nmemb: usize) -> usize {
        let bytes_to_read = size.saturating_mul(nmemb);
        if bytes_to_read == 0 {
            return 0;
        }
        let remaining = &self.data[self.pos..];
        let bytes_read = remaining.len().min(bytes_to_read);
        unsafe {
            std::ptr::copy_nonoverlapping(remaining.as_ptr(), ptr as *mut u8, bytes_read);
        }
        self.pos = self.pos.saturating_add(bytes_read);
        bytes_read
    }

    fn close(&mut self) -> c_int {
        0
    }
}

unsafe extern "C" fn read_func_impl(
    ptr: *mut c_void,
    size: usize,
    nmemb: usize,
    datasource: *mut c_void,
) -> usize {
    if let Some(ds) = (datasource as *mut DataSource).as_mut() {
        ds.read(ptr, size, nmemb)
    } else {
        0
    }
}

unsafe extern "C" fn seek_func_impl(datasource: *mut c_void, offset: i64, origin: c_int) -> c_int {
    if let Some(ds) = (datasource as *mut DataSource).as_mut() {
        ds.seek(offset, origin)
    } else {
        -1
    }
}

unsafe extern "C" fn close_func_impl(datasource: *mut c_void) -> c_int {
    if let Some(ds) = (datasource as *mut DataSource).as_mut() {
        ds.close()
    } else {
        -1
    }
}

#[derive(Debug, Clone, Copy)]
pub struct VideoInfo {
    pub width: i32,
    pub height: i32,
    pub fps: f64,
    pub fmt: i32,
}

pub struct TheoraFile {
    handle: *mut TfHandle,
    datasource: Box<DataSource>,
    info: VideoInfo,
}

impl TheoraFile {
    pub fn open_from_memory(data: Vec<u8>) -> Result<Self> {
        let mut datasource = Box::new(DataSource::new(data));
        let io = tf_callbacks {
            read_func: Some(read_func_impl),
            seek_func: Some(seek_func_impl),
            close_func: Some(close_func_impl),
        };

        let handle =
            unsafe { tfh_open_callbacks(datasource.as_mut() as *mut _ as *mut c_void, io) };
        if handle.is_null() {
            return Err(anyhow!("tf_open_callbacks failed"));
        }

        let has_video = unsafe { tfh_hasvideo(handle) };
        if has_video == 0 {
            unsafe { tfh_close(handle) };
            return Err(anyhow!("no video stream in ogg"));
        }

        let mut width: c_int = 0;
        let mut height: c_int = 0;
        let mut fps: c_double = 0.0;
        let mut fmt: c_int = 0;
        unsafe { tfh_videoinfo(handle, &mut width, &mut height, &mut fps, &mut fmt) };

        Ok(Self {
            handle,
            datasource,
            info: VideoInfo {
                width,
                height,
                fps,
                fmt,
            },
        })
    }

    pub fn info(&self) -> VideoInfo {
        self.info
    }

    pub fn has_audio(&self) -> bool {
        if self.handle.is_null() {
            return false;
        }
        let has = unsafe { tfh_hasaudio(self.handle) };
        has != 0
    }

    pub fn audio_info(&self) -> Option<(i32, i32)> {
        if self.handle.is_null() {
            return None;
        }
        if unsafe { tfh_hasaudio(self.handle) } == 0 {
            return None;
        }
        let mut channels: c_int = 0;
        let mut samplerate: c_int = 0;
        unsafe { tfh_audioinfo(self.handle, &mut channels, &mut samplerate) };
        if channels <= 0 || samplerate <= 0 {
            return None;
        }
        Some((channels, samplerate))
    }

    pub fn reset(&mut self) {
        if self.handle.is_null() {
            return;
        }
        unsafe { tfh_reset(self.handle) };
    }

    pub fn read_video_frame(&mut self, out: &mut [u8]) -> Result<bool> {
        if self.handle.is_null() {
            return Ok(false);
        }
        let eos = unsafe { tfh_eos(self.handle) };
        if eos != 0 {
            return Ok(false);
        }
        if out.is_empty() {
            return Ok(true);
        }
        let ret = unsafe { tfh_readvideo(self.handle, out.as_mut_ptr() as *mut i8, 1) };
        if ret < 0 {
            return Err(anyhow!("tf_readvideo failed"));
        }
        if ret == 0 {
            return Ok(false);
        }
        Ok(true)
    }

    pub fn read_audio_samples(&mut self, out: &mut [f32]) -> Result<usize> {
        if self.handle.is_null() {
            return Ok(0);
        }
        if out.is_empty() {
            return Ok(0);
        }
        let ret = unsafe { tfh_readaudio(self.handle, out.as_mut_ptr(), out.len() as c_int) };
        if ret < 0 {
            return Err(anyhow!("tf_readaudio failed"));
        }
        Ok(ret as usize)
    }
}

impl Drop for TheoraFile {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { tfh_close(self.handle) };
            self.handle = std::ptr::null_mut();
        }
    }
}
