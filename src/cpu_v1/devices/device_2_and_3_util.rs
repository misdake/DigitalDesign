use minifb::{clamp, Key, KeyRepeat, ScaleMode, Window, WindowOptions};
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

fn register_device_2_and_3() {}

struct Tx<T: Send + Sync + Clone> {
    sender: Sender<T>,
    value: T,
}

impl<T: Send + Sync + Clone> Tx<T> {
    fn set_send(&mut self, value: T) {
        self.value = value;
        self.send();
    }
    fn send(&self) {
        self.sender.send(self.value.clone()).unwrap(); //TODO window closed?
    }
}
impl<T: Send + Sync + Clone> Deref for Tx<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}
impl<T: Send + Sync + Clone> DerefMut for Tx<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

struct Rx<T: 'static + Send + Sync + Clone> {
    receiver: Receiver<T>,
    value: T,
}
impl<T: Send + Sync + Clone> Deref for Rx<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}
impl<T: Send + Sync + Clone> DerefMut for Rx<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

impl<T: Send + Sync + Clone> Rx<T> {
    fn update_get(&mut self) -> &T {
        loop {
            match self.receiver.try_recv() {
                Ok(t) => {
                    self.value = t;
                }
                Err(TryRecvError::Empty) => {
                    break;
                }
                Err(TryRecvError::Disconnected) => {
                    break;
                } //TODO window closed?
            }
        }
        &self.value
    }
}

#[derive(Copy, Clone, Hash, Eq, PartialEq)]
enum GamepadButton {
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

struct GamepadState {
    key_mapping: HashMap<Key, GamepadButton>,
    state_prev: HashMap<GamepadButton, i8>,
    state_curr: HashMap<GamepadButton, i8>,
}
impl GamepadState {
    fn new() -> GamepadState {
        let mut state = GamepadState {
            key_mapping: Default::default(),
            state_prev: Default::default(),
            state_curr: Default::default(),
        };
        state.key_mapping.insert(Key::W, GamepadButton::Up);
        state.key_mapping.insert(Key::S, GamepadButton::Up);
        state.key_mapping.insert(Key::A, GamepadButton::Up);
        state.key_mapping.insert(Key::D, GamepadButton::Up);

        state
    }

    fn next_frame(&mut self) {
        self.state_prev = self.state_curr.clone();
    }
    fn update(&mut self, keys: Vec<Key>) {
        //TODO read keys from rx?
        self.state_curr.clear();
    }
    fn get_prev(&self, key: GamepadButton) -> i8 {
        *self.state_prev.get(&key).unwrap_or(&0)
    }
    fn get_curr(&self, key: GamepadButton) -> i8 {
        *self.state_prev.get(&key).unwrap_or(&0)
    }
    fn is_down(&self, key: GamepadButton) -> bool {
        self.get_prev(key) == 0 && self.get_curr(key) == 1
    }
    fn is_pressed(&self, key: GamepadButton) -> bool {
        self.get_curr(key) == 1
    }
    fn is_up(&self, key: GamepadButton) -> bool {
        self.get_prev(key) == 1 && self.get_curr(key) == 0
    }
}

struct MinifbWindow {
    frame_id: Tx<u64>,
    frame_buffer: Rx<FrameBuffer>,
}
#[derive(Clone)]
struct FrameBuffer {
    w: u8,
    h: u8,
    buffer: Vec<u32>,
}
struct MinifbWindowAsyncController {
    frame_id: Rx<u64>,
    frame_buffer: Tx<FrameBuffer>,
}
impl MinifbWindowAsyncController {
    fn get_frame_id(&self) {}
}

enum Control {
    W,
    A,
    S,
    D,
}
struct Submit {
    size: usize,
    buffer: Vec<u32>,
}

struct Controller {
    size: usize,
    x: usize,
    y: usize,

    buffer: Vec<u32>,
    control_rx: Receiver<Control>,
    submit_tx: Sender<Submit>,
}
impl Controller {
    fn create(size: usize, control_rx: Receiver<Control>, submit_tx: Sender<Submit>) -> Self {
        let mut buffer = vec![];
        buffer.resize(size * size, 0);
        Self {
            size,
            x: 0,
            y: 0,
            buffer,
            control_rx,
            submit_tx,
        }
    }

    fn control_then_update(&mut self, c: Control) {
        let mut x = self.x as isize;
        let mut y = self.y as isize;
        match c {
            Control::W => {
                y -= 1;
            }
            Control::S => {
                y += 1;
            }
            Control::A => {
                x -= 1;
            }
            Control::D => {
                x += 1;
            }
        }
        self.x = clamp(0, x, self.size as isize - 1) as usize;
        self.y = clamp(0, y, self.size as isize - 1) as usize;
        self.update();
    }

    fn update(&mut self) {
        self.buffer.fill(0);
        self.buffer[self.size * self.y + self.x] = 0xff0000ff;
        self.submit_tx
            .send(Submit {
                size: self.size,
                buffer: self.buffer.clone(),
            })
            .unwrap();
    }

    fn start(&mut self) {
        loop {
            let result = self.control_rx.try_recv();
            match result {
                Ok(control) => self.control_then_update(control),
                Err(TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(1)),
                Err(TryRecvError::Disconnected) => break,
            }
        }
    }
}

fn start_window() {
    const RENDER_SIZE: usize = 16;
    const SCALE: usize = 32;
    const DISPLAY_SIZE: usize = RENDER_SIZE * SCALE;

    let (control_tx, control_rx) = std::sync::mpsc::channel::<Control>();
    let (submit_tx, submit_rx) = std::sync::mpsc::channel::<Submit>();

    std::thread::spawn(move || {
        let mut controller = Controller::create(RENDER_SIZE, control_rx, submit_tx);
        controller.update();
        controller.start();
    });

    let mut window = Window::new(
        "Window",
        DISPLAY_SIZE,
        DISPLAY_SIZE,
        WindowOptions {
            resize: false,
            scale_mode: ScaleMode::UpperLeft,
            ..WindowOptions::default()
        },
    )
    .expect("Unable to create window");

    window.limit_update_rate(Some(Duration::from_micros(10000)));

    let mut time = Instant::now();

    while window.is_open() && !window.is_key_down(Key::Escape) {
        match submit_rx.try_recv() {
            Ok(v) => {
                assert_eq!(v.size, RENDER_SIZE);
                let mut buf = vec![0u32; DISPLAY_SIZE * DISPLAY_SIZE];
                for i in 0..RENDER_SIZE {
                    let mut line = vec![0u32; DISPLAY_SIZE];
                    for j in 0..RENDER_SIZE {
                        let v = v.buffer[i * RENDER_SIZE + j];
                        line[(j * SCALE)..(j * SCALE) + SCALE].fill(v);
                    }
                    for k in 0..SCALE {
                        let start = i * SCALE * DISPLAY_SIZE + k * RENDER_SIZE * SCALE;
                        let end = i * SCALE * DISPLAY_SIZE + (k + 1) * RENDER_SIZE * SCALE;
                        buf[start..end].copy_from_slice(line.as_slice());
                    }
                }

                window
                    .update_with_buffer(buf.as_slice(), DISPLAY_SIZE, DISPLAY_SIZE)
                    .unwrap();
            }
            Err(_) => {
                window.update();
            }
        }

        window
            .get_keys_pressed(KeyRepeat::Yes)
            .iter()
            .for_each(|key| match key {
                Key::W => control_tx.send(Control::W).unwrap(),
                Key::A => control_tx.send(Control::A).unwrap(),
                Key::S => control_tx.send(Control::S).unwrap(),
                Key::D => control_tx.send(Control::D).unwrap(),
                _ => (),
            });

        // window.get_keys_released().iter().for_each(|key| match key {
        //     Key::W => control_tx.send(Control::W).unwrap(),
        //     Key::A => control_tx.send(Control::A).unwrap(),
        //     Key::S => control_tx.send(Control::S).unwrap(),
        //     Key::D => control_tx.send(Control::D).unwrap(),
        //     _ => (),
        // });

        let time3 = Instant::now();

        println!("frame: buffer {}ms", (time3 - time).as_secs_f32() * 1000.,);

        time = time3;
    }
}
