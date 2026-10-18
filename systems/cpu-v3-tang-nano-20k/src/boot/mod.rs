//! Fixed single-application Flash image and cold-boot functional contract.
//! The v5 format is shared by the packer, OUT_DIR generator and hardware loader.
mod device_abi;
mod devices;
mod single;
pub use device_abi::*;
pub use devices::*;
pub use single::*;
