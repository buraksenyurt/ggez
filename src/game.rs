//! The `game` module contains traits and structs to actually run your game mainloop
//! and handle top-level state.

use context::Context;
use GameResult;
use timer;

use std::time::Duration;

use super::event as gevent;

use sdl2::event::Event::*;
use sdl2::event;
use sdl2::mouse;
use sdl2::keyboard;


/// A trait defining event callbacks.
///
/// The default event handlers do nothing, apart from `key_down_event()`,
/// which *should* by default exit the game if escape is pressed.
/// (Once we work around some event bugs in rust-sdl2.)
pub trait EventHandler {
    /// Called upon each physics update to the game.
    /// This should be where the game's logic takes place.
    fn update(&mut self, ctx: &mut Context, dt: Duration) -> GameResult<()>;

    /// Called to do the drawing of your game.
    /// You probably want to start this with
    /// `graphics::clear()` and end it with
    /// `graphics::present()` and `timer::sleep_until_next_frame()`
    fn draw(&mut self, ctx: &mut Context) -> GameResult<()>;

    // You don't have to override these if you don't want to; the defaults
    // do nothing.
    // It might be nice to be able to have custom event types and a map or
    // such of handlers?  Hmm, maybe later.
    fn mouse_button_down_event(&mut self, _button: mouse::MouseButton, _x: i32, _y: i32) {}

    fn mouse_button_up_event(&mut self, _button: mouse::MouseButton, _x: i32, _y: i32) {}

    fn mouse_motion_event(&mut self,
                          _state: mouse::MouseState,
                          _x: i32,
                          _y: i32,
                          _xrel: i32,
                          _yrel: i32) {
    }

    fn mouse_wheel_event(&mut self, _x: i32, _y: i32) {}

    fn key_down_event(&mut self, _keycode: gevent::Keycode, _keymod: gevent::Mod, _repeat: bool) {}

    fn key_up_event(&mut self, _keycode: gevent::Keycode, _keymod: gevent::Mod, _repeat: bool) {}

    fn controller_button_down_event(&mut self, _btn: gevent::Button) {}
    fn controller_button_up_event(&mut self, _btn: gevent::Button) {}
    fn controller_axis_event(&mut self, _axis: gevent::Axis, _value: i16) {}

    fn focus_event(&mut self, _gained: bool) {}

    /// Called upon a quit event.  If it returns true,
    /// the game does not exit.
    fn quit_event(&mut self) -> bool {
        println!("Quitting game");
        false
    }
}

/// Runs the game's main loop, calling event
/// callbacks on the given state object as events
/// occur.
pub fn run<S>(ctx: &mut Context, state: &mut S) -> GameResult<()>
    where S: EventHandler
{
    {
        let mut event_pump = ctx.sdl_context.event_pump()?;

        let mut continuing = true;
        while continuing {
            ctx.timer_context.tick();

            for event in event_pump.poll_iter() {
                match event {
                    Quit { .. } => {
                        continuing = state.quit_event();
                        // println!("Quit event: {:?}", t);
                    }
                    // TODO: We need a good way to have
                    // a default like this, while still allowing
                    // it to be overridden.
                    // Bah, just put it in the GameState trait
                    // as the default function.
                    // But it doesn't have access to the context
                    // to call quit!  Bah.
                    KeyDown { keycode, keymod, repeat, .. } => {
                        if let Some(key) = keycode {
                            if key == keyboard::Keycode::Escape {
                                ctx.quit()?;
                            } else {
                                state.key_down_event(key, keymod, repeat)
                            }
                        }
                    }
                    KeyUp { keycode, keymod, repeat, .. } => {
                        if let Some(key) = keycode {
                            state.key_up_event(key, keymod, repeat)
                        }
                    }
                    MouseButtonDown { mouse_btn, x, y, .. } => {
                        state.mouse_button_down_event(mouse_btn, x, y)
                    }
                    MouseButtonUp { mouse_btn, x, y, .. } => {
                        state.mouse_button_up_event(mouse_btn, x, y)
                    }
                    MouseMotion { mousestate, x, y, xrel, yrel, .. } => {
                        state.mouse_motion_event(mousestate, x, y, xrel, yrel)
                    }
                    MouseWheel { x, y, .. } => state.mouse_wheel_event(x, y),
                    ControllerButtonDown { button, .. } => {
                        state.controller_button_down_event(button)
                    }
                    ControllerButtonUp { button, .. } => state.controller_button_up_event(button),
                    ControllerAxisMotion { axis, value, .. } => {
                        state.controller_axis_event(axis, value)
                    }
                    Window { win_event: event::WindowEvent::FocusGained, .. } => {
                        state.focus_event(true)
                    }
                    Window { win_event: event::WindowEvent::FocusLost, .. } => {
                        state.focus_event(false)
                    }
                    _ => {}
                }
            }

            // TODO: The catchup_frames is a bit hacky; it might make the
            // problem better but doesn't really fix it, which is basically
            // that this will smooth out hiccups but if your system just can't
            // update fast enough this will only make things worse. Making the
            // number of catchup_frames smaller each time the limit is hit
            // would kinda fix the problem, but also feels like it's starting
            // to  get overly clever.  Might be okay though; need to think
            // about it more.
            // let dt = timer::get_delta(ctx);
            // let mut catchup_frames = 8;
            // {
            //     let mut current_dt = dt + residual_update_dt;
            //     while current_dt > update_dt {
            //         current_dt -= update_dt;
            //         catchup_frames -= 1;
            //         if catchup_frames <= 0 {
            //             break;
            //         }
            //     }
            //     residual_update_dt = current_dt;
            // }
            let dt = timer::get_delta(ctx);
            state.update(ctx, dt)?;
            state.draw(ctx)?;
            timer::sleep(Duration::new(0, 0));
        }
    }

    Ok(())
}
