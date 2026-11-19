//! Device choices persisted by the client; both apply when the device is created.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VertexProcessing {
    #[default]
    Auto,
    Hardware,
    Software,
}
impl VertexProcessing {
    pub const ALL: [Self; 3] = [Self::Auto, Self::Hardware, Self::Software];
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "hardware" => Some(Self::Hardware),
            "software" => Some(Self::Software),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Hardware => "hardware",
            Self::Software => "software",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Hardware => "Hardware",
            Self::Software => "Software",
        }
    }
    #[cfg(any(windows, test))]
    pub(crate) fn software(self, hardware_available: bool) -> Result<bool, &'static str> {
        match self {
            Self::Auto => Ok(!hardware_available),
            Self::Software => Ok(true),
            Self::Hardware if hardware_available => Ok(false),
            Self::Hardware => {
                Err("D3D9: hardware vertex processing requested but HWTRANSFORMANDLIGHT is absent")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorDepth {
    #[default]
    Bits16,
    Bits32,
}
impl ColorDepth {
    pub const ALL: [Self; 2] = [Self::Bits16, Self::Bits32];
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "16" => Some(Self::Bits16),
            "32" => Some(Self::Bits32),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Bits16 => "16",
            Self::Bits32 => "32",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Preferences {
    pub vertex_processing: VertexProcessing,
    pub color_depth: ColorDepth,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn software_can_override_hardware_without_requiring_it() {
        assert_eq!(VertexProcessing::Auto.software(true), Ok(false));
        assert_eq!(VertexProcessing::Auto.software(false), Ok(true));
        assert_eq!(VertexProcessing::Software.software(true), Ok(true));
        assert_eq!(VertexProcessing::Software.software(false), Ok(true));
        assert_eq!(VertexProcessing::Hardware.software(true), Ok(false));
        assert!(VertexProcessing::Hardware.software(false).is_err());
    }
}
