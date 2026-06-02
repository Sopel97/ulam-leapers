pub mod grid_explorer;
mod grid_render;
mod simulation_creator;
mod simulation_runner;

use crate::gui::simulation_creator::SimulationCreator;
use eframe::egui::Ui;
use eframe::{Frame, egui};

pub enum SubwindowResult {
    Keep(Box<dyn Subwindow>),
    Spawn((Box<dyn Subwindow>, Vec<Box<dyn Subwindow>>)),
    Replace(Box<dyn Subwindow>),
    Close,
}

pub trait Subwindow {
    fn name(&self) -> String;
    fn ui(self: Box<Self>, ui: &mut Ui) -> SubwindowResult;
    fn is_closeable(&self) -> bool {
        true
    }
    fn on_close(&mut self) {}
}

#[derive(Default, Clone, Copy, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct TabId(usize);

pub struct TabIdAllocator {
    next_id: usize,
}

impl TabIdAllocator {
    pub fn new() -> Self {
        Self { next_id: 1 }
    }

    pub fn invalid_id() -> TabId {
        TabId(0)
    }

    pub fn next(&mut self) -> TabId {
        let curr = self.next_id;
        self.next_id += 1;
        TabId(curr)
    }
}

#[derive(Default)]
enum SubwindowState {
    #[default]
    Closed,
    Active(Box<dyn Subwindow>),
}

pub struct Tab {
    id: TabId,
    subwindow: SubwindowState,
}

pub struct State {
    tabs: Vec<Tab>,
    tab_id_allocator: TabIdAllocator,
    selected_tab_id: TabId,
}

impl Default for State {
    fn default() -> Self {
        Self {
            tabs: vec![],
            tab_id_allocator: TabIdAllocator::new(),
            selected_tab_id: TabIdAllocator::invalid_id(),
        }
    }
}

pub struct App {
    state: State,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, frame: &mut Frame) {
        let mut tabs_to_spawn: Vec<Box<dyn Subwindow>> = vec![];

        self.drop_closed_tabs();

        egui::Panel::top("main_panel")
            .frame(egui::Frame::new().inner_margin(4))
            .show_inside(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.menu_button("New", |ui| {
                        if ui.button("Creator").clicked() {
                            tabs_to_spawn.push(Box::new(SimulationCreator::new()));
                        }
                        if ui.button("Explorer").clicked() {
                            // TODO: load from file, spawn explorer
                        }
                    });

                    ui.visuals_mut().button_frame = false;
                    self.tab_bar(ui, frame)
                });
            });

        for tab in tabs_to_spawn {
            self.add_tab(tab);
        }

        self.drop_closed_tabs();

        egui::CentralPanel::no_frame().show_inside(ui, |ui| {
            self.process_selected_tab(ui, frame);
        });
    }
}

impl App {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let mut slf = Self {
            state: State::default(),
        };

        slf.add_tab(Box::new(SimulationCreator::new()));

        slf
    }

    pub fn drop_closed_tabs(&mut self) {
        self.state.tabs.retain(|tab| !matches!(tab.subwindow, SubwindowState::Closed));
    }

    pub fn add_tab(&mut self, subwindow: Box<dyn Subwindow>) {
        let id = self.state.tab_id_allocator.next();
        self.state.tabs.push(Tab {
            id,
            subwindow: SubwindowState::Active(subwindow),
        });
        if self.state.selected_tab_id == TabIdAllocator::invalid_id() {
            self.state.selected_tab_id = id;
        }
    }

    pub fn tab_bar(&mut self, ui: &mut Ui, _frame: &mut Frame) {
        let mut selected_tab_id = self.state.selected_tab_id;
        for tab in self.state.tabs.iter_mut() {
            let mut do_close = false;
            match tab.subwindow {
                SubwindowState::Active(ref mut subwindow) => {
                    ui.separator();

                    if ui
                        .selectable_label(selected_tab_id == tab.id, subwindow.name())
                        .clicked()
                    {
                        selected_tab_id = tab.id;
                    }

                    if subwindow.is_closeable()
                        && tab.id == self.state.selected_tab_id
                        && ui.small_button("✖").clicked()
                    {
                        subwindow.on_close();
                        do_close = true;
                    }
                }
                SubwindowState::Closed => {}
            }

            if do_close {
                tab.subwindow = SubwindowState::Closed;
            }
        }

        self.state.selected_tab_id = selected_tab_id;
    }

    pub fn process_selected_tab(&mut self, ui: &mut Ui, _frame: &mut Frame) {
        let mut selected_tab = None;
        for tab in self.state.tabs.iter_mut() {
            if tab.id == self.state.selected_tab_id {
                selected_tab = Some(tab);
                break;
            }
        }

        if let Some(tab) = selected_tab {
            let mut pending_children = vec![];
            let subwindow = std::mem::take(&mut tab.subwindow);
            tab.subwindow = match subwindow {
                SubwindowState::Active(subwindow) => {
                    match subwindow.ui(ui) {
                        SubwindowResult::Keep(kept) => {
                            SubwindowState::Active(kept)
                        }
                        SubwindowResult::Spawn((kept, mut children)) => {
                            pending_children.append(&mut children);
                            SubwindowState::Active(kept)
                        }
                        SubwindowResult::Replace(replacement) => {
                            // Same as Kept, but it's valuable to have a syntactic distinction.
                            SubwindowState::Active(replacement)
                        }
                        SubwindowResult::Close => {
                            SubwindowState::Closed
                        }
                    }
                },
                SubwindowState::Closed => SubwindowState::Closed
            };

            for pending_child in pending_children {
                self.add_tab(pending_child);
            }
        }
    }
}
