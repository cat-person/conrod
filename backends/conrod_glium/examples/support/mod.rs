#![allow(dead_code)]

use glium::{
    glutin::{event, event_loop},
    Display,
};

use conrod_core::event::Input;
use conrod_core::input::touch::Phase;
use conrod_core::input::Key;
use glium::glutin::dpi::{LogicalPosition, LogicalSize};
use glium::glutin::event::{
    ElementState, Event, MouseButton, MouseScrollDelta, Touch, TouchPhase, VirtualKeyCode,
    WindowEvent,
};

use glium::glutin::window::Window;

pub enum Request<'a, 'b: 'a> {
    Event {
        event: &'a Event<'b, ()>,
        should_update_ui: &'a mut bool,
        should_exit: &'a mut bool,
    },
    SetUi {
        needs_redraw: &'a mut bool,
    },
    Redraw,
}

/// In most of the examples the `glutin` crate is used for providing the window context and
/// events while the `glium` crate is used for displaying `conrod_core::render::Primitives` to the
/// screen.
///
/// This function simplifies some of the boilerplate involved in limiting the redraw rate in the
/// glutin+glium event loop.
pub fn run_loop<F>(display: Display, event_loop: event_loop::EventLoop<()>, mut callback: F) -> !
where
    F: 'static + FnMut(Request, &Display),
{
    let sixteen_ms = std::time::Duration::from_millis(16);
    let mut next_update = None;
    let mut ui_update_needed = false;
    event_loop.run(move |event, _, control_flow| {
        {
            let mut should_update_ui = false;
            let mut should_exit = false;
            callback(
                Request::Event {
                    event: &event,
                    should_update_ui: &mut should_update_ui,
                    should_exit: &mut should_exit,
                },
                &display,
            );
            ui_update_needed |= should_update_ui;
            if should_exit {
                *control_flow = event_loop::ControlFlow::Exit;
                return;
            }
        }

        // We don't want to draw any faster than 60 FPS, so set the UI only on every 16ms, unless:
        // - this is the very first event, or
        // - we didn't request update on the last event and new events have arrived since then.
        let should_set_ui_on_main_events_cleared = next_update.is_none() && ui_update_needed;
        match (&event, should_set_ui_on_main_events_cleared) {
            (Event::NewEvents(event::StartCause::Init { .. }), _)
            | (Event::NewEvents(event::StartCause::ResumeTimeReached { .. }), _)
            | (Event::MainEventsCleared, true) => {
                next_update = Some(std::time::Instant::now() + sixteen_ms);
                ui_update_needed = false;

                let mut needs_redraw = false;
                callback(
                    Request::SetUi {
                        needs_redraw: &mut needs_redraw,
                    },
                    &display,
                );
                if needs_redraw {
                    display.gl_window().window().request_redraw();
                } else {
                    // We don't need to redraw anymore until more events arrives.
                    next_update = None;
                }
            }
            _ => {}
        }
        if let Some(next_update) = next_update {
            *control_flow = event_loop::ControlFlow::WaitUntil(next_update);
        } else {
            *control_flow = event_loop::ControlFlow::Wait;
        }

        // Request redraw if needed.
        match &event {
            Event::RedrawRequested(_) => {
                callback(Request::Redraw, &display);
            }
            _ => {}
        }
    })
}

pub fn convert_event(given_event: &Event<()>, window: &Window) -> Option<Input> {
    let scale_factor: f64 = window.scale_factor();
    let (win_w, win_h): (f64, f64) = window.inner_size().to_logical::<f64>(scale_factor).into();

    // Translate the coordinates from top-left-origin-with-y-down to centre-origin-with-y-up.
    let tx = |x: conrod_core::Scalar| x - win_w / 2.0;
    let ty = |y: conrod_core::Scalar| -(y - win_h / 2.0);

    // Functions for converting keys and mouse buttons.
    let map_key = |key: VirtualKeyCode| convert_key(&key);
    let map_mouse = |button: MouseButton| convert_mouse_button(&button);

    match given_event {
        Event::WindowEvent { event, .. } => match event {
            WindowEvent::Resized(physical_size) => {
                let LogicalSize { width, height } = physical_size.to_logical(scale_factor);
                Some(Input::Resize(width, height).into())
            }
            WindowEvent::ReceivedCharacter(ch) => {
                let string = match ch {
                    // Ignore control characters and return ascii for Text event (like sdl2).
                    '\u{7f}' | // Delete
                    '\u{1b}' | // Escape
                    '\u{8}' | // Backspace
                    '\r' | '\n' | '\t' => "".to_string(),
                    _ => ch.to_string()
                };
                Some(Input::Text(string).into())
            }
            WindowEvent::Focused(focused) => {
                Some(Input::Focus(focused.clone()).into())
            }
            WindowEvent::KeyboardInput { input, .. } => {
                input.virtual_keycode.map(|key| match input.state {
                    ElementState::Pressed => Input::Press(
                        conrod_core::input::Button::Keyboard(map_key(key)),
                    )
                    .into(),
                    ElementState::Released => Input::Release(
                        conrod_core::input::Button::Keyboard(map_key(key)),
                    )
                    .into(),
                })
            }

            WindowEvent::Touch(Touch {
                phase,
                location,
                id,
                ..
            }) => {
                let LogicalPosition { x, y } = location.to_logical::<f64>(scale_factor);
                let phase = match phase {
                    TouchPhase::Started => Phase::Start,
                    TouchPhase::Moved => Phase::Move,
                    TouchPhase::Cancelled => Phase::Cancel,
                    TouchPhase::Ended => Phase::End,
                };
                let xy = [tx(x), ty(y)];
                let id = conrod_core::input::touch::Id::new(id.clone());
                let touch = conrod_core::input::Touch {
                    phase: phase,
                    id: id,
                    xy: xy,
                };
                Some(Input::Touch(touch).into())
            }

            WindowEvent::CursorMoved { position, .. } => {
                let LogicalPosition { x, y } = position.to_logical::<f64>(scale_factor);
                let x = tx(x as conrod_core::Scalar);
                let y = ty(y as conrod_core::Scalar);
                let motion = conrod_core::input::Motion::MouseCursor { x: x, y: y };
                Some(Input::Motion(motion).into())
            }

            WindowEvent::MouseWheel { delta, .. } => match delta {
                MouseScrollDelta::PixelDelta(delta) => {
                    let LogicalPosition { x, y } = delta.to_logical::<f64>(scale_factor);
                    let x = x as conrod_core::Scalar;
                    let y = -y as conrod_core::Scalar;
                    let motion = conrod_core::input::Motion::Scroll { x: x, y: y };
                    Some(Input::Motion(motion).into())
                }

                MouseScrollDelta::LineDelta(x, y) => {
                    // This should be configurable (we should provide a LineDelta event to allow for this).
                    const ARBITRARY_POINTS_PER_LINE_FACTOR: conrod_core::Scalar = 10.0;
                    let x = ARBITRARY_POINTS_PER_LINE_FACTOR * x.clone() as conrod_core::Scalar;
                    let y = ARBITRARY_POINTS_PER_LINE_FACTOR * -y.clone() as conrod_core::Scalar;
                    Some(
                        Input::Motion(conrod_core::input::Motion::Scroll {
                            x: x,
                            y: y,
                        })
                        .into(),
                    )
                }
            },

            WindowEvent::MouseInput { state, button, .. } => match state {
                ElementState::Pressed => Some(
                    Input::Press(conrod_core::input::Button::Mouse(map_mouse(
                        button.clone(),
                    )))
                    .into(),
                ),
                ElementState::Released => Some(
                    Input::Release(conrod_core::input::Button::Mouse(
                        map_mouse(button.clone()),
                    ))
                    .into(),
                ),
            },

            _ => None,
        },
        _ => None,
    }
}

fn convert_key(keycode: &VirtualKeyCode) -> Key {
    match keycode {
        Key0 => Key::D0,
        VirtualKeyCode::Key1 => Key::D1,
        VirtualKeyCode::Key2 => Key::D2,
        VirtualKeyCode::Key3 => Key::D3,
        VirtualKeyCode::Key4 => Key::D4,
        VirtualKeyCode::Key5 => Key::D5,
        VirtualKeyCode::Key6 => Key::D6,
        VirtualKeyCode::Key7 => Key::D7,
        VirtualKeyCode::Key8 => Key::D8,
        VirtualKeyCode::Key9 => Key::D9,
        VirtualKeyCode::A => Key::A,
        VirtualKeyCode::B => Key::B,
        VirtualKeyCode::C => Key::C,
        VirtualKeyCode::D => Key::D,
        VirtualKeyCode::E => Key::E,
        VirtualKeyCode::F => Key::F,
        VirtualKeyCode::G => Key::G,
        VirtualKeyCode::H => Key::H,
        VirtualKeyCode::I => Key::I,
        VirtualKeyCode::J => Key::J,
        VirtualKeyCode::K => Key::K,
        VirtualKeyCode::L => Key::L,
        VirtualKeyCode::M => Key::M,
        VirtualKeyCode::N => Key::N,
        VirtualKeyCode::O => Key::O,
        VirtualKeyCode::P => Key::P,
        VirtualKeyCode::Q => Key::Q,
        VirtualKeyCode::R => Key::R,
        VirtualKeyCode::S => Key::S,
        VirtualKeyCode::T => Key::T,
        VirtualKeyCode::U => Key::U,
        VirtualKeyCode::V => Key::V,
        VirtualKeyCode::W => Key::W,
        VirtualKeyCode::X => Key::X,
        VirtualKeyCode::Y => Key::Y,
        VirtualKeyCode::Z => Key::Z,
        VirtualKeyCode::Apostrophe => Key::Unknown,
        VirtualKeyCode::Backslash => Key::Backslash,
        VirtualKeyCode::Back => Key::Backspace,
        // K::CapsLock => Key::CapsLock,
        VirtualKeyCode::Delete => Key::Delete,
        VirtualKeyCode::Comma => Key::Comma,
        VirtualKeyCode::Down => Key::Down,
        VirtualKeyCode::End => Key::End,
        VirtualKeyCode::Return => Key::Return,
        VirtualKeyCode::Equals => Key::Equals,
        VirtualKeyCode::Escape => Key::Escape,
        VirtualKeyCode::F1 => Key::F1,
        VirtualKeyCode::F2 => Key::F2,
        VirtualKeyCode::F3 => Key::F3,
        VirtualKeyCode::F4 => Key::F4,
        VirtualKeyCode::F5 => Key::F5,
        VirtualKeyCode::F6 => Key::F6,
        VirtualKeyCode::F7 => Key::F7,
        VirtualKeyCode::F8 => Key::F8,
        VirtualKeyCode::F9 => Key::F9,
        VirtualKeyCode::F10 => Key::F10,
        VirtualKeyCode::F11 => Key::F11,
        VirtualKeyCode::F12 => Key::F12,
        VirtualKeyCode::F13 => Key::F13,
        VirtualKeyCode::F14 => Key::F14,
        VirtualKeyCode::F15 => Key::F15,
        VirtualKeyCode::Numpad0 => Key::NumPad0,
        VirtualKeyCode::Numpad1 => Key::NumPad1,
        VirtualKeyCode::Numpad2 => Key::NumPad2,
        VirtualKeyCode::Numpad3 => Key::NumPad3,
        VirtualKeyCode::Numpad4 => Key::NumPad4,
        VirtualKeyCode::Numpad5 => Key::NumPad5,
        VirtualKeyCode::Numpad6 => Key::NumPad6,
        VirtualKeyCode::Numpad7 => Key::NumPad7,
        VirtualKeyCode::Numpad8 => Key::NumPad8,
        VirtualKeyCode::Numpad9 => Key::NumPad9,
        VirtualKeyCode::NumpadComma | VirtualKeyCode::NumpadDecimal => {
            Key::NumPadDecimal
        }
        VirtualKeyCode::NumpadDivide => Key::NumPadDivide,
        VirtualKeyCode::NumpadMultiply => Key::NumPadMultiply,
        VirtualKeyCode::NumpadSubtract => Key::NumPadMinus,
        VirtualKeyCode::NumpadAdd => Key::NumPadPlus,
        VirtualKeyCode::NumpadEnter => Key::NumPadEnter,
        VirtualKeyCode::NumpadEquals => Key::NumPadEquals,
        VirtualKeyCode::LShift => Key::LShift,
        VirtualKeyCode::LControl => Key::LCtrl,
        VirtualKeyCode::LAlt => Key::LAlt,
        VirtualKeyCode::RShift => Key::RShift,
        VirtualKeyCode::RControl => Key::RCtrl,
        VirtualKeyCode::RAlt => Key::RAlt,
        VirtualKeyCode::Home => Key::Home,
        VirtualKeyCode::Insert => Key::Insert,
        VirtualKeyCode::Left => Key::Left,
        VirtualKeyCode::LBracket => Key::LeftBracket,
        VirtualKeyCode::Minus => Key::Minus,
        VirtualKeyCode::Numlock => Key::NumLockClear,
        VirtualKeyCode::PageDown => Key::PageDown,
        VirtualKeyCode::PageUp => Key::PageUp,
        VirtualKeyCode::Pause => Key::Pause,
        VirtualKeyCode::Period => Key::Period,
        VirtualKeyCode::Right => Key::Right,
        VirtualKeyCode::RBracket => Key::RightBracket,
        VirtualKeyCode::Semicolon => Key::Semicolon,
        VirtualKeyCode::Slash => Key::Slash,
        VirtualKeyCode::Space => Key::Space,
        VirtualKeyCode::Tab => Key::Tab,
        VirtualKeyCode::Up => Key::Up,
        _ => Key::Unknown,
    }
}

fn convert_mouse_button(button: &MouseButton) -> conrod_core::input::MouseButton {
    match button {
        MouseButton::Left => conrod_core::input::MouseButton::Left,
        MouseButton::Right => conrod_core::input::MouseButton::Right,
        MouseButton::Middle => conrod_core::input::MouseButton::Middle,
        MouseButton::Other(0) => conrod_core::input::MouseButton::X1,
        MouseButton::Other(1) => conrod_core::input::MouseButton::X2,
        MouseButton::Other(2) => conrod_core::input::MouseButton::Button6,
        MouseButton::Other(3) => conrod_core::input::MouseButton::Button7,
        MouseButton::Other(4) => conrod_core::input::MouseButton::Button8,
        _ => conrod_core::input::MouseButton::Unknown,
    }
}
