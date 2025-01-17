use std::{path::Path, process::Command, thread, time::Duration};

use egui::{Button, CursorIcon, RichText, Style, TopBottomPanel};
use glium::{
    backend::glutin::Display,
    glutin::{
        event::{ElementState, ModifiersState, MouseScrollDelta, VirtualKeyCode, WindowEvent},
        event_loop::EventLoopProxy,
        window::Fullscreen,
    },
};
use image::imageops::FilterType;

use crate::{min, rect::Rect, util::UserEvent, vec2::Vec2};

pub mod image_view;
use image_view::ImageView;

pub mod image_list;

mod clipboard;

mod color;
mod help;
mod menu_bar;
mod metadata;

pub mod op_queue;
use op_queue::{Op, OpQueue, Output};

pub mod crop;

pub mod load_image;

mod save_image;
use crop::Crop;

mod undo_stack;

mod cache;

mod resize;
use resize::Resize;

use self::undo_stack::UndoFrame;

const TOP_BAR_SIZE: f32 = 26.0;
const BOTTOM_BAR_SIZE: f32 = 27.0;

pub struct App {
    exit: bool,
    delay: Option<Duration>,
    pub image_view: Option<Box<ImageView>>,
    pub size: Vec2<f32>,
    pub position: Vec2<i32>,
    fullscreen: bool,
    pub top_bar_size: f32,
    pub bottom_bar_size: f32,
    proxy: EventLoopProxy<UserEvent>,
    modifiers: ModifiersState,
    mouse_position: Vec2<f32>,
    current_filename: String,
    op_queue: OpQueue,
    pub crop: Box<Crop>,
    resize: Resize,
    help_visible: bool,
    color_visible: bool,
    metadata_visible: bool,
}

impl App {
    pub fn view_available(&self) -> bool {
        !self.op_queue.working() && self.image_view.is_some()
    }

    pub fn poll(&mut self, display: &Display) {
        while let Some((output, stack)) = self.op_queue.poll() {
            match output {
                Output::ImageLoaded(image_data, path) => {
                    stack.clear();
                    self.current_filename = if let Some(path) = &path {
                        self.op_queue.image_list.change_dir(&path);
                        path.file_name().unwrap().to_str().unwrap().to_string()
                    } else {
                        String::new()
                    };

                    let view = Box::new(ImageView::new(display, image_data, path));
                    self.resize
                        .set_size(Vec2::new(view.size.x() as u32, view.size.y() as u32));
                    self.image_view = Some(view);

                    let window_context = display.gl_window();
                    let window = window_context.window();

                    if self.current_filename.is_empty() {
                        window.set_title("Simp");
                    } else {
                        window.set_title(&self.current_filename.to_string());
                    }

                    self.best_fit();
                }
                Output::FlipHorizontal => {
                    self.image_view.as_mut().unwrap().flip_horizontal(display);
                    stack.push(UndoFrame::FlipHorizontal);
                }
                Output::FlipVertical => {
                    self.image_view.as_mut().unwrap().flip_vertical(display);
                    stack.push(UndoFrame::FlipVertical);
                }
                Output::Rotate(dir) => {
                    self.image_view.as_mut().unwrap().rotate(dir);
                    stack.push(UndoFrame::Rotate(dir));
                }
                Output::Resize(mut frames) => {
                    if let Some(ref mut view) = self.image_view {
                        view.swap_frames(&mut frames, display);
                        stack.push(UndoFrame::Resize(frames));
                    }
                    self.best_fit();
                }
                Output::Color(mut frames) => {
                    if let Some(ref mut view) = self.image_view {
                        view.swap_frames(&mut frames, display);
                        stack.push(UndoFrame::Color(frames));
                        view.hue = 0.0;
                        view.contrast = 0.0;
                        view.saturation = 0.0;
                        view.lightness = 0.0;
                    }
                }
                Output::Crop(mut frames, rotation) => {
                    if let Some(ref mut view) = self.image_view {
                        view.rotation = 0;
                        view.swap_frames(&mut frames, display);
                        stack.push(UndoFrame::Crop { frames, rotation })
                    }
                }
                Output::Undo => {
                    let frame = stack.undo();
                    if let Some(frame) = frame {
                        match frame {
                            UndoFrame::Rotate(rot) => {
                                self.image_view.as_mut().unwrap().rotate(-*rot);
                            }
                            UndoFrame::FlipHorizontal => {
                                self.image_view.as_mut().unwrap().flip_horizontal(display);
                            }
                            UndoFrame::FlipVertical => {
                                self.image_view.as_mut().unwrap().flip_vertical(display);
                            }
                            UndoFrame::Crop { frames, rotation } => {
                                let view = self.image_view.as_mut().unwrap();
                                view.swap_frames(frames, display);
                                std::mem::swap(&mut view.rotation, rotation);
                            }
                            UndoFrame::Resize(frames) => {
                                let view = self.image_view.as_mut().unwrap();
                                view.swap_frames(frames, display);
                            }
                            UndoFrame::Color(frames) => {
                                let view = self.image_view.as_mut().unwrap();
                                view.swap_frames(frames, display);
                            }
                        }
                    }
                }
                Output::Redo => {
                    let frame = stack.redo();
                    if let Some(frame) = frame {
                        match frame {
                            UndoFrame::Rotate(rot) => {
                                self.image_view.as_mut().unwrap().rotate(*rot);
                            }
                            UndoFrame::FlipHorizontal => {
                                self.image_view.as_mut().unwrap().flip_horizontal(display);
                            }
                            UndoFrame::FlipVertical => {
                                self.image_view.as_mut().unwrap().flip_vertical(display);
                            }
                            UndoFrame::Crop { frames, rotation } => {
                                let view = self.image_view.as_mut().unwrap();
                                view.swap_frames(frames, display);
                                std::mem::swap(&mut view.rotation, rotation);
                            }
                            UndoFrame::Resize(frames) => {
                                let view = self.image_view.as_mut().unwrap();
                                view.swap_frames(frames, display);
                            }
                            UndoFrame::Color(frames) => {
                                let view = self.image_view.as_mut().unwrap();
                                view.swap_frames(frames, display);
                            }
                        }
                    }
                }
                Output::Close => {
                    self.image_view = None;
                    stack.clear();
                    self.op_queue.image_list.clear();
                    self.crop.cropping = false;
                    self.op_queue.cache.clear();
                }
                // indicates that the operation is done with no output
                Output::Done => (),
            }
        }
    }

    pub fn handle_user_event(&mut self, display: &Display, event: &mut UserEvent) {
        self.poll(display);
        match event {
            UserEvent::QueueLoad(path) => {
                self.queue(Op::LoadPath(path.to_path_buf(), false));
            }
            UserEvent::QueueSave(path) => {
                self.queue(Op::Save(path.to_path_buf()));
            }
            UserEvent::ErrorMessage(error) => {
                let error = error.clone();
                thread::spawn(move || {
                    msgbox::create("Error", &error, msgbox::IconType::Error).unwrap()
                });
            }
            UserEvent::Exit => self.exit = true,
            UserEvent::Wake => (),
        };
    }

    pub fn handle_window_event(&mut self, display: &Display, event: &WindowEvent<'_>) {
        match event {
            WindowEvent::Resized(size) => {
                *self.size.mut_x() = size.width as f32;
                *self.size.mut_y() = size.height as f32;
            }
            WindowEvent::Moved(position) => {
                *self.position.mut_x() = position.x;
                *self.position.mut_x() = position.y;
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_position.set_x(position.x as f32);
                self.mouse_position.set_y(position.y as f32);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if !self.metadata_visible {
                    let scroll = match delta {
                        MouseScrollDelta::LineDelta(_, y) => *y,
                        MouseScrollDelta::PixelDelta(pos) => pos.y as f32,
                    };

                    if self.crop.inner.is_none() {
                        self.zoom(scroll, self.mouse_position);
                    }
                }
            }
            WindowEvent::ModifiersChanged(state) => self.modifiers = *state,
            WindowEvent::DroppedFile(path) => {
                self.op_queue.cache.clear();
                self.queue(Op::LoadPath(path.to_path_buf(), true));
            }
            WindowEvent::KeyboardInput { input, .. } if !self.resize.visible => {
                if let Some(key) = input.virtual_keycode {
                    match input.state {
                        ElementState::Pressed => match key {
                            VirtualKeyCode::Delete => {
                                if let Some(ref view) = self.image_view {
                                    if let Some(ref path) = view.path {
                                        delete(path, self.proxy.clone());
                                    }
                                }
                            }

                            VirtualKeyCode::H if self.modifiers.ctrl() => self.help_visible = true,
                            VirtualKeyCode::O if self.modifiers.ctrl() => {
                                load_image::open(self.proxy.clone(), display)
                            }
                            VirtualKeyCode::S if self.modifiers.ctrl() => save_image::open(
                                self.current_filename.clone(),
                                self.proxy.clone(),
                                display,
                            ),
                            VirtualKeyCode::W if self.modifiers.ctrl() => self.exit = true,
                            VirtualKeyCode::N if self.modifiers.ctrl() => new_window(),

                            VirtualKeyCode::F => {
                                self.largest_fit();
                            }
                            VirtualKeyCode::B => {
                                self.best_fit();
                            }

                            VirtualKeyCode::Q => {
                                if self.image_view.is_some() {
                                    self.queue(Op::Rotate(-1))
                                }
                            }
                            VirtualKeyCode::E => {
                                if self.image_view.is_some() {
                                    self.queue(Op::Rotate(1))
                                }
                            }

                            VirtualKeyCode::F5 => {
                                if let Some(image) = self.image_view.as_ref() {
                                    if let Some(path) = &image.path {
                                        let buf = path.to_path_buf();
                                        self.queue(Op::LoadPath(buf, false));
                                    }
                                }
                            }

                            VirtualKeyCode::C if self.modifiers.ctrl() => {
                                if self.view_available() {
                                    self.queue(Op::Copy);
                                }
                            }
                            VirtualKeyCode::V if self.modifiers.ctrl() => {
                                if !self.op_queue.working() {
                                    self.queue(Op::Paste);
                                }
                            }
                            VirtualKeyCode::X if self.modifiers.ctrl() => {
                                self.crop.cropping = true;
                            }

                            VirtualKeyCode::Z if self.modifiers.ctrl() => {
                                self.queue(Op::Undo);
                            }
                            VirtualKeyCode::Y if self.modifiers.ctrl() => {
                                self.queue(Op::Redo);
                            }

                            VirtualKeyCode::R if self.modifiers.ctrl() => {
                                self.resize.visible = true;
                            }

                            VirtualKeyCode::Left | VirtualKeyCode::D => {
                                if self.crop.inner.is_none() && self.view_available() {
                                    self.queue(Op::Prev);
                                }
                            }

                            VirtualKeyCode::Right | VirtualKeyCode::A => {
                                if self.crop.inner.is_none() && self.view_available() {
                                    self.queue(Op::Next);
                                }
                            }
                            VirtualKeyCode::F4 if self.modifiers.ctrl() => {
                                self.queue(Op::Close);
                            }

                            VirtualKeyCode::F11 => {
                                let window_context = display.gl_window();
                                let window = window_context.window();
                                let fullscreen = window.fullscreen();
                                if fullscreen.is_some() {
                                    window.set_fullscreen(None);
                                    self.fullscreen = false;
                                    self.top_bar_size = TOP_BAR_SIZE;
                                    self.bottom_bar_size = BOTTOM_BAR_SIZE;
                                } else {
                                    window.set_fullscreen(Some(Fullscreen::Borderless(None)));
                                    self.fullscreen = true;
                                    self.top_bar_size = 0.0;
                                    self.bottom_bar_size = 0.0;
                                }
                            }
                            VirtualKeyCode::Escape => {
                                let window_context = display.gl_window();
                                let window = window_context.window();
                                let fullscreen = window.fullscreen();
                                if fullscreen.is_some() {
                                    window.set_fullscreen(None);
                                    self.fullscreen = false;
                                }
                            }
                            _ => (),
                        },
                        ElementState::Released => (),
                    }
                }
            }
            WindowEvent::ReceivedCharacter(c) if !self.resize.visible => match c {
                '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' => {
                    if let Some(ref mut view) = self.image_view {
                        let zoom = c.to_digit(10).unwrap() as f32;
                        view.scale = zoom;
                    }
                }
                '+' => {
                    if self.crop.inner.is_none() {
                        self.zoom(1.0, self.size / 2.0);
                    }
                }
                '-' => {
                    if self.crop.inner.is_none() {
                        self.zoom(-1.0, self.size / 2.0);
                    }
                }
                _ => (),
            },
            _ => (),
        };
    }

    pub fn handle_ui(&mut self, display: &Display, ctx: &egui::Context) {
        if self.op_queue.working() {
            ctx.output().cursor_icon = CursorIcon::Progress;
        } else if self.crop.cropping {
            ctx.output().cursor_icon = CursorIcon::Crosshair;
        }
        if !self.fullscreen {
            self.menu_bar(display, ctx);
            self.bottom_bar(ctx);
        }
        self.main_area(display, ctx);
        self.resize_ui(ctx);
        self.help_ui(ctx);
        self.color_ui(ctx);
        self.metadata_ui(ctx);
    }

    pub fn main_area(&mut self, _display: &Display, ctx: &egui::Context) {
        let frame = egui::Frame::dark_canvas(&Style::default()).multiply_with_opacity(0.0);
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            if self.image_view.is_none() {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new("Open File: Ctrl + O\n\nPaste: Ctrl + V\n\nHelp: Ctrl + H")
                            .size(20.0),
                    );
                });
            }

            let res = ui.interact(egui::Rect::EVERYTHING, ui.id(), egui::Sense::drag());

            if let Some(ref mut image) = self.image_view {
                if res.dragged_by(egui::PointerButton::Primary) {
                    let vec = res.drag_delta();
                    let delta = Vec2::from((vec.x, vec.y));
                    if self.crop.cropping {
                        if let Some(ref mut inner) = self.crop.inner {
                            inner.current += delta;
                        } else {
                            let cursor_pos = self.mouse_position;
                            self.crop.inner = Some(crop::Inner {
                                start: cursor_pos - delta,
                                current: cursor_pos,
                            });
                        }
                    } else {
                        image.position += delta;
                    }
                } else if self.crop.cropping {
                    if let Some(ref inner) = self.crop.inner {
                        let mut size = inner.current - inner.start;
                        *size.mut_x() = size.x().abs();
                        *size.mut_y() = size.y().abs();

                        let start = Vec2::new(
                            min!(inner.start.x(), inner.current.x()),
                            min!(inner.start.y(), inner.current.y()),
                        );

                        self.queue(Op::Crop(Rect::new(start, size)));
                        self.crop.inner = None;
                        self.crop.cropping = false;
                    }
                }
            }
        });
    }

    fn bottom_bar(&mut self, ctx: &egui::Context) {
        TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.with_layout(egui::Layout::left_to_right(), |ui| {
                if self.image_view.is_some() {
                    ui.add_enabled_ui(self.view_available() && !self.crop.cropping, |ui| {
                        if ui.small_button("⬅").clicked() {
                            self.queue(Op::Prev);
                        }
                        if ui.small_button("➡").clicked() {
                            self.queue(Op::Next);
                        }
                    });
                }

                if let Some(image) = self.image_view.as_mut() {
                    ui.label(format!("{} x {}", image.size.x(), image.size.y()));
                    ui.label(format!("Zoom: {}%", (image.scale * 100.0).round()));
                }
            });
        });
    }

    pub fn update(&mut self, display: &Display) -> (bool, Option<Duration>) {
        self.exit = false;
        self.delay = None;

        if let Some(ref mut image) = self.image_view {
            update_delay(&mut self.delay, &image.animate(display));
        }

        if let Some(ref mut image) = self.image_view {
            let image_size = image.real_size();
            let mut window_size = self.size;
            window_size.set_y(window_size.y() - self.top_bar_size - self.bottom_bar_size);

            if image_size.x() < window_size.x() {
                image.position.set_x(self.size.x() / 2.0);
            } else {
                if image.position.x() - image_size.x() / 2.0 > 0.0 {
                    image.position.set_x(image_size.x() / 2.0);
                }

                if image.position.x() + image_size.x() / 2.0 < window_size.x() {
                    image.position.set_x(window_size.x() - image_size.x() / 2.0);
                }
            }

            if image_size.y() < window_size.y() {
                image.position.set_y(self.size.y() / 2.0);
            } else {
                if image.position.y() - image_size.y() / 2.0 > self.top_bar_size {
                    image
                        .position
                        .set_y(image_size.y() / 2.0 + self.top_bar_size);
                }

                if image.position.y() + image_size.y() / 2.0 < window_size.y() + self.top_bar_size {
                    image
                        .position
                        .set_y((window_size.y() - image_size.y() / 2.0) + self.top_bar_size);
                }
            }
        }

        (self.exit, self.delay)
    }

    pub fn resize_ui(&mut self, ctx: &egui::Context) {
        if self.resize.visible {
            let mut open = self.image_view.is_some();
            let mut resized = false;
            egui::Window::new("Resize")
                .id(egui::Id::new("resize window"))
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    egui::Grid::new("resize grid").show(ui, |ui| {
                        ui.with_layout(egui::Layout::right_to_left(), |ui| {
                            ui.label("Width: ");
                        });
                        let w_focus = ui.text_edit_singleline(&mut self.resize.width).has_focus();
                        ui.end_row();
                        ui.with_layout(egui::Layout::right_to_left(), |ui| {
                            ui.label("Height: ");
                        });
                        let h_focus = ui.text_edit_singleline(&mut self.resize.height).has_focus();
                        ui.end_row();

                        self.resize.width.retain(|c| c.is_numeric());
                        self.resize.width.retain(|c| c.is_numeric());

                        ui.with_layout(egui::Layout::right_to_left(), |ui| {
                            ui.label("Maintain aspect ratio: ");
                        });
                        ui.checkbox(&mut self.resize.maintain_aspect_ratio, "");
                        ui.end_row();

                        ui.with_layout(egui::Layout::right_to_left(), |ui| {
                            ui.label("Resample: ");
                        });
                        let selected = &mut self.resize.resample;
                        egui::ComboBox::new("filter", "")
                            .selected_text(filter_name(selected))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    selected,
                                    FilterType::Nearest,
                                    filter_name(&FilterType::Nearest),
                                );
                                ui.selectable_value(
                                    selected,
                                    FilterType::Triangle,
                                    filter_name(&FilterType::Triangle),
                                );
                                ui.selectable_value(
                                    selected,
                                    FilterType::CatmullRom,
                                    filter_name(&FilterType::CatmullRom),
                                );
                                ui.selectable_value(
                                    selected,
                                    FilterType::Gaussian,
                                    filter_name(&FilterType::Gaussian),
                                );
                                ui.selectable_value(
                                    selected,
                                    FilterType::Lanczos3,
                                    filter_name(&FilterType::Lanczos3),
                                );
                            });
                        ui.end_row();
                        ui.end_row();

                        let width = self.resize.width.parse::<u32>();
                        let height = self.resize.height.parse::<u32>();

                        if self.resize.maintain_aspect_ratio && w_focus && width.is_ok() {
                            let width = *width.as_ref().unwrap();
                            let size = self.image_view.as_ref().unwrap().size;
                            let ratio = width as f32 / size.x();
                            self.resize.height = ((ratio * size.y()) as u32).to_string();
                        }

                        if self.resize.maintain_aspect_ratio && h_focus && height.is_ok() {
                            let height = *height.as_ref().unwrap();
                            let size = self.image_view.as_ref().unwrap().size;
                            let ratio = height as f32 / size.y();
                            self.resize.width = ((ratio * size.x()) as u32).to_string();
                        }

                        ui.with_layout(
                            egui::Layout::top_down_justified(egui::Align::Center),
                            |ui| {
                                if ui
                                    .add_enabled(
                                        width.is_ok() && height.is_ok() && self.view_available(),
                                        Button::new("Cancel"),
                                    )
                                    .clicked()
                                {
                                    resized = true;
                                }
                            },
                        );

                        ui.with_layout(
                            egui::Layout::top_down_justified(egui::Align::Center),
                            |ui| {
                                if ui
                                    .add_enabled(
                                        width.is_ok() && height.is_ok() && self.view_available(),
                                        Button::new("Resize"),
                                    )
                                    .clicked()
                                {
                                    let width = width.unwrap();
                                    let height = height.unwrap();
                                    self.queue(Op::Resize(
                                        Vec2::new(width, height),
                                        self.resize.resample,
                                    ));
                                    resized = true;
                                }
                            },
                        );
                    });
                });
            self.resize.visible = open && !resized;
        }
    }

    pub fn queue(&mut self, op: Op) {
        self.op_queue
            .queue(op, self.image_view.as_ref().map(|v| v.as_ref()))
    }

    fn zoom(&mut self, zoom: f32, mouse_position: Vec2<f32>) {
        if let Some(ref mut image) = self.image_view {
            let old_scale = image.scale;
            image.scale += image.scale * zoom as f32 / 10.0;

            let new_size = image.scaled();
            if (new_size.x() < 100.0 || new_size.y() < 100.0)
                && old_scale >= image.scale
                && image.scale < 1.0
            {
                image.scale = min!(old_scale, 1.0);
            } else {
                let mouse_to_center = image.position - mouse_position;
                image.position -= mouse_to_center * (old_scale - image.scale) / old_scale;
            }
        }
    }

    pub fn best_fit(&mut self) {
        if let Some(ref mut view) = self.image_view {
            let scaling = min!(
                self.size.x() / view.size.x(),
                (self.size.y() - self.top_bar_size - self.bottom_bar_size) / view.size.y()
            );
            view.scale = min!(scaling, 1.0);
            view.position = self.size / 2.0;
        }
    }

    pub fn largest_fit(&mut self) {
        if let Some(ref mut view) = self.image_view {
            let scaling = min!(
                self.size.x() / view.size.x(),
                (self.size.y() - self.top_bar_size - self.bottom_bar_size) / view.size.y()
            );
            view.scale = scaling;
            view.position = self.size / 2.0;
        }
    }

    pub fn new(
        proxy: EventLoopProxy<UserEvent>,
        size: [f32; 2],
        position: [i32; 2],
        display: &Display,
    ) -> Self {
        App {
            exit: false,
            delay: None,
            image_view: None,
            size: Vec2::from(size),
            position: Vec2::from(position),
            fullscreen: false,
            top_bar_size: TOP_BAR_SIZE,
            bottom_bar_size: BOTTOM_BAR_SIZE,
            op_queue: OpQueue::new(proxy.clone()),
            proxy,
            modifiers: ModifiersState::empty(),
            mouse_position: Vec2::default(),
            current_filename: String::new(),
            crop: Box::new(Crop::new(display)),
            resize: Resize::default(),
            help_visible: false,
            color_visible: false,
            metadata_visible: false,
        }
    }
}

pub fn delete<P: AsRef<Path>>(path: P, proxy: EventLoopProxy<UserEvent>) {
    let path = path.as_ref().to_path_buf();
    thread::spawn(move || {
        let dialog = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title("Move to trash")
            .set_description("Are you sure you want to move this to trash?")
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();

        if dialog {
            if let Err(error) = trash::delete(path) {
                let _ = proxy.send_event(UserEvent::ErrorMessage(error.to_string()));
            }
        }
    });
}

fn new_window() {
    let _ = Command::new(std::env::current_exe().unwrap()).spawn();
}

fn update_delay(old: &mut Option<Duration>, new: &Option<Duration>) {
    if let Some(ref mut old_time) = old {
        if let Some(ref new_time) = new {
            if *old_time > *new_time {
                *old_time = *new_time;
            }
        }
    } else {
        *old = *new;
    }
}

fn filter_name(filter: &FilterType) -> &'static str {
    match filter {
        FilterType::Nearest => "Nearest Neighbor",
        FilterType::Triangle => "Linear Filter",
        FilterType::CatmullRom => "Cubic Filter",
        FilterType::Gaussian => "Gaussian Filter",
        FilterType::Lanczos3 => "Lanczos",
    }
}
