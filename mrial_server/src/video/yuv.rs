#[cfg(any(target_os = "linux", target_os = "macos"))]
use libyuv_sys::{ARGBToI444, ARGBToJ420};

pub enum EColorSpace {
    YUV444 = 12,
    YUV422 = 7,
}

impl Into<usize> for EColorSpace {
    fn into(self) -> usize {
        self as usize
    }
}

pub struct YUVBuffer {
    yuv: Vec<u8>,
    width: usize,
    height: usize,
}

impl YUVBuffer {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            yuv: vec![0u8; (3 * (width * height)) / 2],
            width,
            height,
        }
    }

    pub fn with_bgra_for_420(width: usize, height: usize, bgra: &[u8]) -> Self {
        let mut rval = Self {
            yuv: vec![0u8; (3 * width * height) / 2],
            width,
            height,
        };

        rval.read_bgra_for_420(bgra);
        rval
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn with_bgra_for_422(width: usize, height: usize, bgra: &[u8]) -> Self {
        let mut rval = Self {
            yuv: vec![0u8; 2 * (width * height)],
            width,
            height,
        };

        rval.read_bgra_for_422(bgra);
        rval
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn with_bgra_for_444(width: usize, height: usize, bgra: &[u8]) -> Self {
        let mut rval = Self {
            yuv: vec![0u8; 3 * width * height],
            width,
            height,
        };

        rval.read_bgra_for_444(bgra);
        rval
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn read_bgra_for_444(&mut self, bgra: &[u8]) {
        assert_eq!(bgra.len(), self.width * self.height * 4);
        assert_eq!(self.width % 2, 0, "width needs to be multiple of 2");
        assert_eq!(self.height % 2, 0, "height needs to be a multiple of 2");

        let u = self.width * self.height;
        let v = u + u;
        let dst_stride_y = self.width;
        let dst_stride_uv = self.width;
        let dst_y = self.yuv.as_mut_ptr();
        let dst_u = self.yuv[u..].as_mut_ptr();
        let dst_v = self.yuv[v..].as_mut_ptr();

        unsafe {
            ARGBToI444(
                bgra.as_ptr(),
                (bgra.len() / self.height) as _,
                dst_y,
                dst_stride_y as _,
                dst_u,
                dst_stride_uv as _,
                dst_v,
                dst_stride_uv as _,
                self.width as _,
                self.height as _,
            );
        }
    }

    #[cfg(target_os = "windows")]
    pub fn read_bgra_for_420(&mut self, bgra: &[u8]) {}

    #[cfg(target_os = "windows")]
    pub fn read_bgra_for_422(&mut self, bgra: &[u8]) {}

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn read_bgra_for_420(&mut self, bgra: &[u8]) {
        assert_eq!(bgra.len(), self.width * self.height * 4);
        assert_eq!(self.width % 2, 0, "width needs to be multiple of 2");
        assert_eq!(self.height % 2, 0, "height needs to be a multiple of 2");

        let u = self.width * self.height;
        let v = u + u / 4;
        let dst_stride_y = self.width;
        let dst_stride_uv = self.width / 2;
        let dst_y = self.yuv.as_mut_ptr();
        let dst_u = self.yuv[u..].as_mut_ptr();
        let dst_v = self.yuv[v..].as_mut_ptr();

        unsafe {
            ARGBToJ420(
                bgra.as_ptr(),
                (bgra.len() / self.height) as _,
                dst_y,
                dst_stride_y as _,
                dst_u,
                dst_stride_uv as _,
                dst_v,
                dst_stride_uv as _,
                self.width as _,
                self.height as _,
            );
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn read_bgra_for_422(&mut self, bgra: &[u8]) {
        use libyuv_sys::ARGBToJ422;

        assert_eq!(bgra.len(), self.width * self.height * 4);
        assert_eq!(self.width % 2, 0, "width needs to be multiple of 2");
        assert_eq!(self.height % 2, 0, "height needs to be a multiple of 2");

        let u = self.width * self.height;
        let v = u + u / 2;
        let dst_stride_y = self.width;
        let dst_stride_uv = self.width / 2;
        let dst_y = self.yuv.as_mut_ptr();
        let dst_u = self.yuv[u..].as_mut_ptr();
        let dst_v = self.yuv[v..].as_mut_ptr();

        unsafe {
            ARGBToJ422(
                bgra.as_ptr(),
                (bgra.len() / self.height) as _,
                dst_y,
                dst_stride_y as _,
                dst_u,
                dst_stride_uv as _,
                dst_v,
                dst_stride_uv as _,
                self.width as _,
                self.height as _,
            );
        }
    }

    pub fn y(&self) -> &[u8] {
        &self.yuv[0..self.width * self.height]
    }

    fn u_420(&self) -> &[u8] {
        let base_u = self.width * self.height;
        &self.yuv[base_u..base_u + base_u / 4]
    }

    fn v_420(&self) -> &[u8] {
        let base_u = self.width * self.height;
        let base_v = base_u + base_u / 4;
        &self.yuv[base_v..]
    }

    pub fn u_422(&self) -> &[u8] {
        let base_u = self.width * self.height;
        &self.yuv[base_u..base_u + base_u / 2]
    }

    pub fn v_422(&self) -> &[u8] {
        let base_u = self.width * self.height;
        let base_v = base_u + base_u / 2;
        &self.yuv[base_v..]
    }

    pub fn u_444(&self) -> &[u8] {
        let base_u = self.width * self.height;
        &self.yuv[base_u..base_u + base_u]
    }

    pub fn v_444(&self) -> &[u8] {
        let base_u = self.width * self.height;
        let base_v = base_u + base_u;
        &self.yuv[base_v..]
    }
}
