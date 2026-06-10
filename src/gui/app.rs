use crate::gui::grid_explorer::GridExplorer;
use crate::gui::simulation_creator::SimulationCreator;
use crate::gui::subwindow::{Subwindow, SubwindowResult};
use eframe::egui::{Button, Color32, PointerButton, Sense, Ui, Widget};
use eframe::{Frame, egui};
use std::path::PathBuf;

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

    fn try_open_simulations(
        paths: Vec<PathBuf>,
    ) -> Vec<Result<Box<dyn Subwindow>, std::io::Error>> {
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
                            let paths = rfd::FileDialog::new()
                                .add_filter("Ulam Leapers Simulation", &["uls"])
                                .pick_files();
                            if let Some(paths) = paths {
                                let results = Self::try_open_simulations(paths);
                                for result in results {
                                    match result {
                                        Ok(subwindow) => {
                                            tabs_to_spawn.push(subwindow);
                                        }
                                        Err(err) => {
                                            eprintln!("Error opening simulation: {}", err);
                                        }
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
        self.state
            .tabs
            .retain(|tab| !matches!(tab.subwindow, SubwindowState::Closed));
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

    fn tab_bar(&mut self, ui: &mut Ui, _frame: &mut Frame) {
        let mut selected_tab_id = self.state.selected_tab_id;
        let mut tab_labels = vec![];

        for tab in self.state.tabs.iter_mut() {
            let mut do_close = false;
            match tab.subwindow {
                SubwindowState::Active(ref mut subwindow) => {
                    ui.separator();

                    // We want to show tab id for disambiguation.
                    let name = format!("({}) {}", tab.id.0, subwindow.name());
                    let tab_label_widget = Button::selectable(selected_tab_id == tab.id, name)
                        .sense(Sense::click() | Sense::drag());

                    let tab_label = ui.add(tab_label_widget);
                    if tab_label.clicked() {
                        selected_tab_id = tab.id;
                    }

                    if subwindow.is_closeable() {
                        if (tab.id == self.state.selected_tab_id && ui.small_button("✖").clicked())
                            || tab_label.clicked_by(PointerButton::Middle)
                        {
                            subwindow.on_close();
                            do_close = true;
                        }
                    } else {
                        // TODO: somehow make it not display the X.
                        ui.add_enabled(false, Button::new("✖").small());
                    }

                    tab_labels.push((tab.id, tab_label));
                }
                SubwindowState::Closed => {}
            }

            if do_close {
                tab.subwindow = SubwindowState::Closed;
            }
        }

        self.state.selected_tab_id = selected_tab_id;

        // Process tab movement. We can only do this now because we need
        // positions of all tabs to determine which ones to swap.
        let mut swap_tabs = None;
        for (tab_id, label) in &tab_labels {
            let is_dragging = label.dragged_by(PointerButton::Primary);
            let stopped_dragging = label.drag_stopped_by(PointerButton::Primary);

            if is_dragging || stopped_dragging {
                let mouse_pos = ui.ctx().input(|i| i.pointer.latest_pos());
                if let Some(mouse_pos) = mouse_pos {
                    // We know there is a first tab. We need to swap with it if the
                    // user drags beyond it to the left.
                    let mut swap_candidate = &tab_labels[0];

                    for e in &tab_labels {
                        let (_, label_other) = e;
                        if mouse_pos.x > label_other.rect.min.x
                            && mouse_pos.y > label_other.rect.min.y
                        {
                            swap_candidate = e;
                        }
                    }

                    // Highlight target tab.
                    let mut painter = ui.painter_at(swap_candidate.1.rect);
                    painter.set_opacity(0.5);
                    painter.rect_filled(swap_candidate.1.rect, 3, Color32::GREEN);

                    if stopped_dragging {
                        swap_tabs = Some((*tab_id, swap_candidate.0));
                    }
                }
            }
        }

        // Apply the tab swap if any. Needs to be done out of tab iteration.
        if let Some((lhs_tab_id, rhs_tab_id)) = swap_tabs.take()
            && let Some(lhs_tab_pos) = self.state.tabs.iter().position(|tab| tab.id == lhs_tab_id)
            && let Some(rhs_tab_pos) = self.state.tabs.iter().position(|tab| tab.id == rhs_tab_id)
        {
            self.state.tabs.swap(lhs_tab_pos, rhs_tab_pos);
        }
    }

    fn process_tabs(&mut self, ui: &mut Ui, _frame: &mut Frame) {
        let mut pending_children = vec![];
        for tab in self.state.tabs.iter_mut() {
            let is_selected = tab.id == self.state.selected_tab_id;

            let subwindow = std::mem::take(&mut tab.subwindow);
            tab.subwindow = match subwindow {
                SubwindowState::Active(subwindow) => {
                    let cmd = if is_selected {
                        subwindow.ui(ui)
                    } else {
                        subwindow.not_ui(ui.ctx())
                    };
                    match cmd {
                        SubwindowResult::Keep(kept) => SubwindowState::Active(kept),
                        SubwindowResult::Spawn((kept, mut children)) => {
                            pending_children.append(&mut children);
                            SubwindowState::Active(kept)
                        }
                        SubwindowResult::Replace(replacement) => {
                            // Same as Kept, but it's valuable to have a syntactic distinction.
                            SubwindowState::Active(replacement)
                        }
                        SubwindowResult::Close => SubwindowState::Closed,
                    }
                }
                SubwindowState::Closed => SubwindowState::Closed,
            };
        }

        for pending_child in pending_children {
            self.add_tab(pending_child);
        }
    }
}
