use minifb::{clamp, Key, KeyRepeat, ScaleMode, Window, WindowOptions};
use std::any::Any;
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

fn register_device_2_and_3() {}

trait Message: Any + Send + Sync + PartialEq + Clone {}
impl<T: Any + Send + Sync + PartialEq + Clone> Message for T {}

struct Tx<T: Message> {
    sender: Sender<T>,
}

impl<T: Message> Tx<T> {
    fn send(&mut self, value: T) {
        self.sender.send(value).unwrap();
    }
}

struct Rx<T: Message> {
    receiver: Receiver<T>,
    value: T,
}
struct SharedRxWithDiff<T: Message> {
    inner: Arc<RwLock<Rx<T>>>,
    value: T,
}

impl<T: Message> Rx<T> {
    fn update_get(&mut self) -> &T {
        self.update_get_check().0
    }
    fn update_get_check(&mut self) -> (&T, bool) {
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
impl<T: Message> SharedRxWithDiff<T> {
    fn update_get_check<R>(&mut self, f: impl FnOnce(&T, bool) -> R) -> R {
        let mut guard = self.inner.write().unwrap();
        let (value, _) = guard.update_get_check();
        let updated = *value != self.value;
        f(value, !updated)
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
    frame_id: SharedRxWithDiff<u64>,
    keys: Rx<Vec<Key>>,
    mapping: HashMap<Key, GamepadButton>,
    state_prev: HashMap<GamepadButton, i8>,
    state_curr: HashMap<GamepadButton, i8>,
}
impl GamepadState {
    pub fn new(frame_id: SharedRxWithDiff<u64>, keys: Rx<Vec<Key>>) -> GamepadState {
        let mut state = GamepadState {
            frame_id,
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
    pub fn update(&mut self) {
        self.frame_id.update_get_check(|_, updated| {
            if updated {
                std::mem::swap(&mut self.state_prev, &mut self.state_curr);
                self.state_curr.clear();
            }
        });
        let (keys, changed) = self.keys.update_get_check();
        if changed {
            self.state_curr.clear();
            for key in keys {
                self.mapping.get(key).map(|key| {
                    self.state_curr.insert(*key, 1);
                });
            }
        }
    }
    fn get_prev(&self, key: GamepadButton) -> i8 {
        *self.state_prev.get(&key).unwrap_or(&0)
    }
    fn get_curr(&self, key: GamepadButton) -> i8 {
        *self.state_prev.get(&key).unwrap_or(&0)
    }
    pub fn is_down(&self, key: GamepadButton) -> bool {
        self.get_prev(key) == 0 && self.get_curr(key) == 1
    }
    pub fn is_pressed(&self, key: GamepadButton) -> bool {
        self.get_curr(key) == 1
    }
    pub fn is_up(&self, key: GamepadButton) -> bool {
        self.get_prev(key) == 1 && self.get_curr(key) == 0
    }
}

struct MinifbWindow {
    frame_id: Tx<u64>,
    frame_buffer: Rx<FrameBuffer>,
    gamepad: Tx<Vec<Key>>,
}
#[derive(Clone, PartialEq)]
struct FrameBuffer {
    w: u8,
    h: u8,
    buffer: Vec<u32>,
}
struct FrameBufferController {
    frame_id: SharedRxWithDiff<u64>,
    frame_buffer: Tx<FrameBuffer>,
}
impl FrameBufferController {
    pub fn new(frame_id: SharedRxWithDiff<u64>, frame_buffer: Tx<FrameBuffer>) -> Self {
        Self {
            frame_id,
            frame_buffer,
        }
    }
    pub fn get_frame_id(&mut self) -> u64 {
        self.frame_id.update_get_check(|id, _| *id)
    }
    pub fn send_framebuffer(&mut self, framebuffer: FrameBuffer) {
        self.frame_buffer.send(framebuffer);
    }
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
