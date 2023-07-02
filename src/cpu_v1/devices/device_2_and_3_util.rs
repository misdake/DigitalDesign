use minifb::{Key, ScaleMode, Window, WindowOptions};
use std::any::Any;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::time::Duration;

pub trait Message: Any + Default + Send + Sync + PartialEq + Clone {}
impl<T: Any + Default + Send + Sync + PartialEq + Clone> Message for T {}

fn create_channel<T: Message>() -> (Tx<T>, Rx<T>) {
    let (sender, receiver) = channel();
    (
        Tx { sender },
        Rx {
            receiver,
            value: T::default(),
        },
    )
}

pub struct Tx<T: Message> {
    sender: Sender<T>,
}
impl<T: Message> Tx<T> {
    pub fn send(&mut self, value: T) {
        self.sender.send(value).unwrap();
    }
}

pub struct Rx<T: Message> {
    receiver: Receiver<T>,
    value: T,
}
unsafe impl<T: Message> Send for Rx<T> {}
impl<T: Message> Rx<T> {
    pub fn update_get(&mut self) -> &T {
        self.update_get_check().0
    }
    pub fn update_get_check(&mut self) -> (&T, bool) {
        let mut updated = false;
        loop {
            match self.receiver.try_recv() {
                Ok(t) => {
                    self.value = t;
                    updated = true;
                }
                Err(TryRecvError::Empty) => {
                    break;
                }
                Err(TryRecvError::Disconnected) => {
                    break;
                } //TODO window closed?
            }
        }
        (&self.value, updated)
    }
}

pub struct RxWithDiff<T: Message> {
    inner: Rx<T>,
    value: T,
}
unsafe impl<T: Message> Send for RxWithDiff<T> {}
impl<T: Message> RxWithDiff<T> {
    fn new(rx: Rx<T>) -> RxWithDiff<T> {
        Self {
            inner: rx,
            value: T::default(),
        }
    }
    fn update_get_diff_local<R>(&mut self, f: impl FnOnce(&T, bool) -> R) -> R {
        let (value, _) = self.inner.update_get_check();
        let updated = *value != self.value;
        self.value = value.clone();
        f(value, updated)
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub enum GamepadButton {
    Up = 1,
    Down,
    Left,
    Right,
    A,
    B,
    X,
    Y,
    LB,
    RB,
    Start,
    Option,
}
// #[derive(Copy, Clone, Hash, Eq, PartialEq)]
// enum GamepadAnalog {
//     LT,
//     RT,
//     LX,
//     LY,
//     RX,
//     RY,
// }

pub struct GamepadState {
    keys: Rx<Vec<Key>>,
    mapping: HashMap<Key, GamepadButton>,
    state_prev: HashMap<GamepadButton, i8>,
    state_curr: HashMap<GamepadButton, i8>,
}
impl GamepadState {
    pub fn new(keys: Rx<Vec<Key>>) -> GamepadState {
        let mut state = GamepadState {
            keys,
            mapping: Default::default(),
            state_prev: Default::default(),
            state_curr: Default::default(),
        };
        state.mapping.insert(Key::W, GamepadButton::Up);
        state.mapping.insert(Key::S, GamepadButton::Down);
        state.mapping.insert(Key::A, GamepadButton::Left);
        state.mapping.insert(Key::D, GamepadButton::Right);
        state.mapping.insert(Key::J, GamepadButton::A);
        state.mapping.insert(Key::K, GamepadButton::B);
        state.mapping.insert(Key::L, GamepadButton::X);
        state.mapping.insert(Key::Semicolon, GamepadButton::Y);
        state.mapping.insert(Key::Enter, GamepadButton::Start);
        state.mapping.insert(Key::LeftShift, GamepadButton::Option);

        state
    }
    pub fn next_frame(&mut self) {
        self.state_prev = self.state_curr.clone();
    }
    pub fn update(&mut self) {
        let (keys, changed) = self.keys.update_get_check();
        if changed {
            self.state_curr.clear();
            for key in keys {
                self.mapping.get(key).map(|key| {
                    self.state_curr.insert(*key, 1);
                });
            }
        }

        // let prev = self
        //     .state_prev
        //     .keys()
        //     .map(|key| format!("{:?}", key))
        //     .collect::<Vec<_>>()
        //     .join(",");
        // let curr = self
        //     .state_curr
        //     .keys()
        //     .map(|key| format!("{:?}", key))
        //     .collect::<Vec<_>>()
        //     .join(",");
        // println!("keys: prev {prev}, curr {curr}");
    }
    fn get_prev(&self, button: GamepadButton) -> i8 {
        *self.state_prev.get(&button).unwrap_or(&0)
    }
    fn get_curr(&self, button: GamepadButton) -> i8 {
        *self.state_curr.get(&button).unwrap_or(&0)
    }
    pub fn is_down(&self, button: GamepadButton) -> bool {
        self.get_prev(button) == 0 && self.get_curr(button) == 1
    }
    pub fn is_pressed(&self, button: GamepadButton) -> bool {
        self.get_curr(button) == 1
    }
    pub fn is_up(&self, button: GamepadButton) -> bool {
        self.get_prev(button) == 1 && self.get_curr(button) == 0
    }
}

pub fn create_window() -> (MinifbWindow, FrameBufferController, GamepadState) {
    let frame_id = create_channel::<u64>();
    let frame_buffer = create_channel::<FrameBuffer>();
    let gamepad = create_channel::<Vec<Key>>();
    let frame_id_rx = RxWithDiff::new(frame_id.1);

    let window = MinifbWindow {
        frame_id: frame_id.0,
        frame_buffer: frame_buffer.1,
        gamepad: gamepad.0,
    };
    let frame_buffer_controller = FrameBufferController {
        frame_id: frame_id_rx,
        frame_buffer: frame_buffer.0,
    };
    let gamepad_state = GamepadState::new(gamepad.1);
    (window, frame_buffer_controller, gamepad_state)
}

pub struct MinifbWindow {
    frame_id: Tx<u64>,
    frame_buffer: Rx<FrameBuffer>,
    gamepad: Tx<Vec<Key>>,
}
#[derive(Default, Clone, PartialEq)]
pub struct FrameBuffer {
    w: u8,
    h: u8,
    buffer: Vec<u32>,
}
pub struct FrameBufferController {
    frame_id: RxWithDiff<u64>,
    frame_buffer: Tx<FrameBuffer>,
}
impl FrameBufferController {
    pub fn new(frame_id: RxWithDiff<u64>, frame_buffer: Tx<FrameBuffer>) -> Self {
        Self {
            frame_id,
            frame_buffer,
        }
    }
    pub fn get_frame_id(&mut self) -> u64 {
        self.frame_id.update_get_diff_local(|id, _| *id)
    }
    pub fn send_framebuffer(&mut self, framebuffer: FrameBuffer) {
        self.frame_buffer.send(framebuffer);
    }
}

impl MinifbWindow {
    pub fn start_event_loop(mut self, width: usize, height: usize) {
        let mut window = Window::new(
            "Window",
            width,
            height,
            WindowOptions {
                resize: false,
                scale_mode: ScaleMode::AspectRatioStretch,
                ..WindowOptions::default()
            },
        )
        .expect("Unable to create window");

        window.limit_update_rate(Some(Duration::from_micros(10000)));

        // let mut time = std::time::Instant::now();
        let mut frame_id = 1;
        self.frame_id.send(frame_id);
        self.gamepad.sender.send(window.get_keys()).unwrap();

        while window.is_open() && !window.is_key_down(Key::Escape) {
            let (buffer, updated) = self.frame_buffer.update_get_check();
            if updated {
                println!("frame receive buffer");
                window
                    .update_with_buffer(
                        buffer.buffer.as_slice(),
                        buffer.w as usize,
                        buffer.h as usize,
                    )
                    .unwrap();
                frame_id += 1;
                self.frame_id.send(frame_id);
            } else {
                window.update();
            }

            self.gamepad.sender.send(window.get_keys()).unwrap();

            // let time3 = std::time::Instant::now();
            // println!("frame: buffer {}ms", (time3 - time).as_secs_f32() * 1000.,);
            // time = time3;
        }
    }
}

#[test]
fn start_test_window() {
    struct Game {
        gamepad: GamepadState,
        framebuffer: FrameBufferController,

        size: i8,
        x: i8,
        y: i8,
    }
    impl Game {
        pub fn new(size: u8, gamepad: GamepadState, framebuffer: FrameBufferController) -> Self {
            Self {
                gamepad,
                framebuffer,
                size: size as i8,
                x: 2,
                y: 2,
            }
        }
        pub fn start(&mut self) {
            // game loop
            loop {
                // input
                self.gamepad.update();
                if self.gamepad.is_down(GamepadButton::Left) {
                    self.x -= 1;
                    self.x = self.x.clamp(0, self.size - 1);
                }
                if self.gamepad.is_down(GamepadButton::Right) {
                    self.x += 1;
                    self.x = self.x.clamp(0, self.size - 1);
                }
                if self.gamepad.is_down(GamepadButton::Up) {
                    self.y -= 1;
                    self.y = self.y.clamp(0, self.size - 1);
                }
                if self.gamepad.is_down(GamepadButton::Down) {
                    self.y += 1;
                    self.y = self.y.clamp(0, self.size - 1);
                }

                // draw buffer
                fn from_u8_rgb(r: u8, g: u8, b: u8) -> u32 {
                    let (r, g, b) = (r as u32, g as u32, b as u32);
                    (r << 16) | (g << 8) | b
                }
                let mut buffer: Vec<u32> = vec![];
                for y in 0..self.size {
                    for x in 0..self.size {
                        if x == self.x && y == self.y {
                            buffer.push(from_u8_rgb(255, 0, 0));
                        } else {
                            buffer.push(from_u8_rgb(255, 255, 255))
                        }
                    }
                }

                // present
                self.framebuffer.send_framebuffer(FrameBuffer {
                    w: self.size as u8,
                    h: self.size as u8,
                    buffer,
                });
                self.gamepad.next_frame();

                // wait for next frame
                loop {
                    let frame_updated = self
                        .framebuffer
                        .frame_id
                        .update_get_diff_local(|_, changed| changed);
                    if frame_updated {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }

    let (fbwindow, framebuffer, gamepad) = create_window();

    const RENDER_SIZE: u8 = 5;
    const DISPLAY_SIZE: usize = 512;

    std::thread::spawn(move || {
        let mut game = Game::new(RENDER_SIZE, gamepad, framebuffer);
        game.start();
    });

    fbwindow.start_event_loop(DISPLAY_SIZE, DISPLAY_SIZE);
}
