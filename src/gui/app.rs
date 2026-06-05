use std::path::PathBuf;
use eframe::egui::{Button, Ui};
use eframe::{Frame, egui};
use crate::gui::grid_explorer::GridExplorer;
use crate::gui::simulation_creator::SimulationCreator;
use crate::gui::subwindow::{Subwindow, SubwindowResult};

#[derive(Default)]
enum SubwindowState {
    #[default]
    Closed,
    Active(Box<dyn Subwindow>),
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

impl App {
    fn try_open_simulation(path: PathBuf) -> Result<Box<dyn Subwindow>, std::io::Error> {
        GridExplorer::load_from_file(path).map(|v| Box::new(v) as Box<dyn Subwindow>)
    }

    fn try_open_simulations(paths: Vec<PathBuf>) -> Vec<Result<Box<dyn Subwindow>, std::io::Error>> {
        paths.into_iter().map(Self::try_open_simulation).collect()
    }
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
                            let paths = rfd::FileDialog::new().add_filter("Ulam Leapers Simulation", &["uls"]).pick_files();
                            if let Some(paths) = paths {
                                let results = Self::try_open_simulations(paths);
                                for result in results {
                                    match result {
                                        Ok(subwindow) => { tabs_to_spawn.push(subwindow); }
                                        Err(err) => { eprintln!("Error opening simulation: {}", err); }
                                    }
                                }
                            }
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
            self.process_tabs(ui, frame);
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
                    {
                        if ui.small_button("✖").clicked() {
                            subwindow.on_close();
                            do_close = true;
                        }
                    } else {
                        // TODO: somehow make it not display the X.
                        ui.add_enabled(false, Button::new("✖").small());
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

    pub fn process_tabs(&mut self, ui: &mut Ui, _frame: &mut Frame) {
        let mut pending_children = vec![];
        for tab in self.state.tabs.iter_mut() {
            let is_selected = tab.id == self.state.selected_tab_id;

            let subwindow = std::mem::take(&mut tab.subwindow);
            tab.subwindow = match subwindow {
                SubwindowState::Active(subwindow) => {
                    let cmd = if is_selected { subwindow.ui(ui) } else { subwindow.not_ui(ui.ctx()) };
                    match cmd {
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
        }

        for pending_child in pending_children {
            self.add_tab(pending_child);
        }
    }
}
