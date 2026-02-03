use ddc::Ddc;
use ddc_i2c::{I2cDeviceDdc, I2cDeviceEnumerator};

const BRIGHTNESS: u8 = 0x10;

pub struct Monitor {
    handle: I2cDeviceDdc,
}

impl Monitor {
    pub fn get_brightness(&mut self) -> u16 {
        self.handle
            .get_vcp_feature(BRIGHTNESS)
            .map(|v| v.value())
            .unwrap_or(0)
    }

    pub fn set_brightness(&mut self, new_val: u16) {
        if !(0..=100).contains(&new_val) {
            return;
        }

        let _ = self.handle.set_vcp_feature(BRIGHTNESS, new_val);
        let _ = self.handle.save_current_settings();
    }
}

impl From<I2cDeviceDdc> for Monitor {
    fn from(handle: I2cDeviceDdc) -> Self {
        Self { handle }
    }
}

pub fn get_all_monitors() -> Vec<Monitor> {
    let Ok(enumerator) = I2cDeviceEnumerator::new() else {
        return vec![];
    };

    enumerator.map(Monitor::from).collect()
}
