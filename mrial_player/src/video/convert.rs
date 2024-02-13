use libyuv_sys::I444ToRGB24;

pub struct RGBBuffer {
    rgb: Vec<u8>,
    width: usize,
    height: usize,
}

impl RGBBuffer {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn with_444_for_rgb24(
        width: usize, 
        height: usize, 
        y: &[u8], 
        u: &[u8],
        v: &[u8]
    ) -> Self {
        let mut rval = Self {
            rgb: vec![0u8; 3 * width * height],
            width,
            height,
        };

        rval.read_444_for_rgb24(y, u, v);
        rval
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn read_444_for_rgb24(&mut self, y: &[u8], u: &[u8], v: &[u8]) {
        assert_eq!(y.len(), self.width * self.height);
        assert_eq!(u.len(), self.width * self.height);
        assert_eq!(v.len(), self.width * self.height);

        unsafe {
            I444ToRGB24(
                y.as_ptr(),
                self.width as _,
                v.as_ptr(),
                self.width as _,
                u.as_ptr(),
                self.width as _,
                self.rgb.as_mut_ptr(),
                (self.width * 3) as _,
                self.width as _,
                self.height as _,
            );
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.rgb
    }
}