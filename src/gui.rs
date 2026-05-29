pub mod grid_explorer;

use eframe::egui::Ui;
use eframe::{Frame, egui};
use crate::gui::grid_explorer::GridExplorer;

pub trait Tab {
    fn name(&self) -> String;
    fn ui(&mut self, ui: &mut egui::Ui);
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

pub struct State {
    tabs: Vec<(TabId, Box<dyn Tab>)>,
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

        slf.add_tab(Box::new(GridExplorer::new()));

        slf
    }

    pub fn add_tab(&mut self, tab: Box<dyn Tab>) {
        let id = self.state.tab_id_allocator.next();
        self.state.tabs.push((id, tab));
        if self.state.selected_tab_id == TabIdAllocator::invalid_id() {
            self.state.selected_tab_id = id;
        }
    }

    pub fn tab_bar(&mut self, ui: &mut egui::Ui, _frame: &mut Frame) {
        let mut selected_tab_id = self.state.selected_tab_id;
        for (id, tab) in self.state.tabs.iter_mut() {
            if ui
                .selectable_label(selected_tab_id == *id, tab.name())
                .clicked()
            {
                selected_tab_id = *id;
            }
        }
        self.state.selected_tab_id = selected_tab_id;
    }

    pub fn show_selected_tab(&mut self, ui: &mut egui::Ui, _frame: &mut Frame) {
        for (id, tab) in self.state.tabs.iter_mut() {
            if *id == self.state.selected_tab_id {
                tab.ui(ui);
            }
        }
    }
}
