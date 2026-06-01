pub mod grid_explorer;
mod grid_render;
mod simulation_creator;

use crate::gui::simulation_creator::SimulationCreator;
use eframe::egui::Ui;
use eframe::{Frame, egui};

pub trait Subwindow {
    fn name(&self) -> String;
    fn ui(&mut self, ui: &mut Ui);
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

pub struct Tab {
    id: TabId,
    is_closed: bool,
    subwindow: Box<dyn Subwindow>,
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
        self.drop_closed_tabs();

        egui::Panel::top("main_panel")
            .frame(egui::Frame::new().inner_margin(4))
            .show_inside(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.visuals_mut().button_frame = false;
                    self.tab_bar(ui, frame)
                });
            });

        egui::CentralPanel::no_frame().show_inside(ui, |ui| {
            self.show_selected_tab(ui, frame);
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
        for tab in &mut self.state.tabs {
            if tab.is_closed {
                tab.subwindow.on_close();
            }
        }
        self.state.tabs.retain(|t| !t.is_closed);
    }

    pub fn add_tab(&mut self, subwindow: Box<dyn Subwindow>) {
        let id = self.state.tab_id_allocator.next();
        self.state.tabs.push(Tab {
            id,
            subwindow,
            is_closed: false,
        });
        if self.state.selected_tab_id == TabIdAllocator::invalid_id() {
            self.state.selected_tab_id = id;
        }
    }

    pub fn tab_bar(&mut self, ui: &mut Ui, _frame: &mut Frame) {
        let mut selected_tab_id = self.state.selected_tab_id;
        for tab in self.state.tabs.iter_mut() {
            ui.separator();

            if ui
                .selectable_label(selected_tab_id == tab.id, tab.subwindow.name())
                .clicked()
            {
                selected_tab_id = tab.id;
            }

            if tab.subwindow.is_closeable()
                && tab.id == self.state.selected_tab_id
                && ui.small_button("✖").clicked()
            {
                tab.is_closed = true;
            }
            ui.separator();
        }
        self.state.selected_tab_id = selected_tab_id;
    }

    pub fn show_selected_tab(&mut self, ui: &mut Ui, _frame: &mut Frame) {
        for tab in self.state.tabs.iter_mut() {
            if tab.id == self.state.selected_tab_id {
                tab.subwindow.ui(ui);
            }
        }
    }
}
