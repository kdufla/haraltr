use evdev::{
    AttributeSet, EventType, InputEvent, KeyCode, RelativeAxisCode, uinput::VirtualDevice,
};

pub struct VirtualKeyboard(VirtualDevice);

impl VirtualKeyboard {
    pub fn new() -> Result<Self, std::io::Error> {
        let mut keys = AttributeSet::new();
        keys.insert(KeyCode::KEY_ENTER);

        let device = VirtualDevice::builder()?
            .name("haraltr-keyboard")
            .with_keys(&keys)?
            .build()?;

        Ok(Self(device))
    }

    pub fn press_enter(&mut self) -> Result<(), std::io::Error> {
        self.0.emit(&[
            InputEvent::new(EventType::KEY.0, KeyCode::KEY_ENTER.0, 1),
            InputEvent::new(EventType::KEY.0, KeyCode::KEY_ENTER.0, 0),
        ])
    }
}

pub struct VirtualMouse(VirtualDevice);

impl VirtualMouse {
    pub fn new() -> Result<Self, std::io::Error> {
        let mut axes = AttributeSet::new();
        axes.insert(RelativeAxisCode::REL_X);

        let device = VirtualDevice::builder()?
            .name("haraltr-mouse")
            .with_relative_axes(&axes)?
            .build()?;

        Ok(Self(device))
    }

    pub fn move_mouse(&mut self) -> Result<(), std::io::Error> {
        self.0.emit(&[
            InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_X.0, 1),
            InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_X.0, -1),
        ])
    }
}
