use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "i2c bus {0} not found; check `ls /dev/i2c-*` and pick the bus the panel is on with --bus"
    )]
    BusNotFound(PathBuf),

    #[error(
        "no permission to open {0}; add yourself to the i2c group (`sudo usermod -aG i2c $USER`) and log in again"
    )]
    BusPermission(PathBuf),

    #[error("could not open {0}: {1}")]
    BusOpen(PathBuf, String),

    #[error(
        "display at {addr:#04x} on {bus} did not respond ({detail}); list the devices on the bus with `i2cdetect -y -r <bus number>`"
    )]
    Display {
        bus: PathBuf,
        addr: u8,
        detail: String,
    },

    #[error("cannot read disk usage of {0}; pass an existing mount point with --disk")]
    Disk(PathBuf),

    #[error("could not install the signal handler: {0}")]
    Signal(std::io::Error),
}
