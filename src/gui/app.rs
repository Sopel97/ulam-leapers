use crate::gui::grid_explorer::GridExplorer;
use crate::gui::simulation_creator::SimulationCreator;
use crate::gui::simulation_resumer::SimulationResumer;
use crate::gui::subwindow::{Subwindow, SubwindowResult};
use eframe::egui::{Button, Color32, PointerButton, Response, Sense, Ui};
use eframe::{egui, Frame};
use std::collections::BTreeMap;
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

impl Default for TabIdAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl TabIdAllocator {
    pub fn new() -> Self {
        Self { next_id: 1 }
    }

    pub fn invalid_id() -> TabId {
        TabId(0)
    }

    pub fn next_tab_id(&mut self) -> TabId {
        let curr = self.next_id;
        self.next_id += 1;
        TabId(curr)
    }
}

pub struct Tab {
    id: TabId,
    subwindow: SubwindowState,
    highlight_until_selected: bool,
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
                        if ui.button("Empty Creator").clicked() {
                            tabs_to_spawn.push(Box::new(SimulationCreator::new()));
                        }
                        if ui.button("Creator from ULS").on_hover_text("Opens a new creator with player configuration loaded from a ULS file").clicked() {
                            let path = rfd::FileDialog::new()
                                .add_filter("Ulam Leapers Simulation", &["uls"])
                                .pick_file();
                            if let Some(path) = path {
                                let result = SimulationCreator::new_from_uls(path);
                                match result {
                                    Ok(creator) => {
                                        tabs_to_spawn.push(Box::new(creator));
                                    }
                                    Err(err) => {
                                        eprintln!("Error opening creator: {}", err);
                                    }
                                }
                            }
                        }
                        if ui.button("Continue from ULS").clicked() {
                            let path = rfd::FileDialog::new()
                                .add_filter("Ulam Leapers Simulation", &["uls"])
                                .pick_file();
                            if let Some(path) = path {
                                let result = SimulationResumer::load_from_file(path);
                                match result {
                                    Ok(resumer) => {
                                        tabs_to_spawn.push(Box::new(resumer));
                                    }
                                    Err(err) => {
                                        eprintln!("Error opening resumer: {}", err);
                                    }
                                }
                            }
                        }
                        if ui.button("Explorer from ULS").on_hover_text("Opens a new explorer from a ULS file").clicked() {
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
        let id = self.state.tab_id_allocator.next_tab_id();
        self.state.tabs.push(Tab {
            id,
            subwindow: SubwindowState::Active(subwindow),
            highlight_until_selected: true,
        });
        if self.state.selected_tab_id == TabIdAllocator::invalid_id() {
            self.state.selected_tab_id = id;
        }
    }

    fn make_tab_names(tabs: &[Tab]) -> BTreeMap<TabId, String> {
        let subwindow_names_by_tab_id = tabs
            .iter()
            .flat_map(|tab| match &tab.subwindow {
                SubwindowState::Active(subwindow) => Some((tab.id, subwindow.name())),
                SubwindowState::Closed => None,
            })
            .collect::<Vec<_>>();

        // We want to show tab id for disambiguation, but only if necessary.
        subwindow_names_by_tab_id
            .iter()
            .map(|(tab_id, subwindow_name)| {
                let num_subwindow_name_occurrences = subwindow_names_by_tab_id
                    .iter()
                    .filter(|v| v.1 == *subwindow_name)
                    .count();

                let tab_name = if num_subwindow_name_occurrences > 1 {
                    format!("({}) {}", tab_id.0, subwindow_name)
                } else {
                    subwindow_name.clone()
                };

                (*tab_id, tab_name)
            })
            .collect::<BTreeMap<_, _>>()
    }

    fn tab_bar(&mut self, ui: &mut Ui, _frame: &mut Frame) {
        let mut selected_tab_id = self.state.selected_tab_id;
        let mut tab_labels = vec![];

        let tab_names = Self::make_tab_names(&self.state.tabs);

        for tab in self.state.tabs.iter_mut() {
            let mut do_close = false;
            match tab.subwindow {
                SubwindowState::Active(ref mut subwindow) => {
                    ui.separator();

                    let tab_name = tab_names.get(&tab.id).expect(
                        "`make_tab_names` should have been called with the same set of tabs.",
                    );

                    let tab_label_widget = Button::selectable(selected_tab_id == tab.id, tab_name)
                        .sense(Sense::click() | Sense::drag());

                    let tab_label = ui.add(tab_label_widget);

                    if subwindow.highligh_until_selected() {
                        tab.highlight_until_selected = true;
                    }

                    if tab_label.clicked() {
                        selected_tab_id = tab.id;
                    }

                    // Highlight tabs that the user has not yet discovered
                    if tab.highlight_until_selected {
                        let mut painter = ui.painter_at(tab_label.rect);
                        painter.set_opacity(0.25);
                        painter.rect_filled(tab_label.rect, 3, Color32::GREEN);
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

        self.process_tab_movement(ui, tab_labels.as_slice());
    }

    fn process_tab_movement(&mut self, ui: &mut Ui, tab_labels: &[(TabId, Response)]) {
        // Process tab movement. We can only do this now because we need
        // positions of all tabs to determine which ones to swap.
        let mut swap_tabs = None;
        for (tab_id, label) in tab_labels {
            let is_dragging = label.dragged_by(PointerButton::Primary);
            let stopped_dragging = label.drag_stopped_by(PointerButton::Primary);

            if is_dragging || stopped_dragging {
                let mouse_pos = ui.ctx().input(|i| i.pointer.latest_pos());
                if let Some(mouse_pos) = mouse_pos {
                    // We know there is a first tab. We need to swap with it if the
                    // user drags beyond it to the left.
                    let mut swap_candidate = &tab_labels[0];

                    for e in tab_labels {
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
                        tab.highlight_until_selected = false;
                        subwindow.ui(ui)
                    } else {
                        subwindow.not_ui(ui.ctx())
                    };
                    match cmd {
                        SubwindowResult::Keep(kept) => SubwindowState::Active(kept),
                        SubwindowResult::KeepAndHighlightUntilSelected(kept) => {
                            tab.highlight_until_selected = true;
                            ui.ctx().request_repaint();
                            SubwindowState::Active(kept)
                        }
                        SubwindowResult::Spawn((kept, mut children)) => {
                            pending_children.append(&mut children);
                            ui.ctx().request_repaint();
                            SubwindowState::Active(kept)
                        }
                        SubwindowResult::Replace(replacement) => {
                            // Same as Kept, but it's valuable to have a syntactic distinction.
                            tab.highlight_until_selected = true;
                            ui.ctx().request_repaint();
                            SubwindowState::Active(replacement)
                        }
                        SubwindowResult::Close => {
                            ui.ctx().request_repaint();
                            SubwindowState::Closed
                        }
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
