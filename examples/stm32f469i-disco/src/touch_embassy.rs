use embassy_stm32::i2c::{self, I2c};

const FT6X06_ADDR: u8 = 0x38;
const REG_TD_STATUS: u8 = 0x02;
const REG_TOUCH1_XH: u8 = 0x03;

pub struct TouchPoint {
    pub x: u16,
    pub y: u16,
}

pub struct TouchCtrl {
    i2c_addr: u8,
}

impl TouchCtrl {
    pub fn new() -> Self {
        Self {
            i2c_addr: FT6X06_ADDR,
        }
    }

    pub fn td_status(
        &self,
        i2c: &mut i2c::I2c<'_, embassy_stm32::mode::Blocking, i2c::Master>,
    ) -> Result<u8, ()> {
        let mut buf = [0u8; 1];
        i2c.blocking_write_read(self.i2c_addr, &[REG_TD_STATUS], &mut buf)
            .map_err(|_| ())?;
        Ok(buf[0] & 0x0F)
    }

    pub fn get_touch(
        &self,
        i2c: &mut i2c::I2c<'_, embassy_stm32::mode::Blocking, i2c::Master>,
    ) -> Result<TouchPoint, ()> {
        let mut buf = [0u8; 4];
        i2c.blocking_write_read(self.i2c_addr, &[REG_TOUCH1_XH], &mut buf)
            .map_err(|_| ())?;

        let x = (((buf[0] & 0x0F) as u16) << 8) | (buf[1] as u16);
        let y = (((buf[2] & 0x0F) as u16) << 8) | (buf[3] as u16);
        Ok(TouchPoint { x, y })
    }

    pub fn read_chip_id(
        &self,
        i2c: &mut i2c::I2c<'_, embassy_stm32::mode::Blocking, i2c::Master>,
    ) -> Result<u8, ()> {
        let mut buf = [0u8; 1];
        i2c.blocking_write_read(self.i2c_addr, &[0xA8], &mut buf)
            .map_err(|_| ())?;
        Ok(buf[0])
    }
}
