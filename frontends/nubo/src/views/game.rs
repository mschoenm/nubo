//! The game screen: a bar with the game's controls and the Game Boy screen,
//! and the pause menu with its save state pages. In a window the bar sits
//! above the picture. In fullscreen the picture fills the screen and the bar
//! appears along the bottom when the mouse moves; at the top it would be
//! hidden behind the menu bar that macOS slides down there. With `--scale`
//! the window shows the picture alone ([`screen_only`]).

use crate::app::{GameMessage, Message, SettingsMessage, WindowMessage};
use crate::game::{Game, PauseMenu};
use crate::settings::Settings;
use crate::ui::icons::{Icon, icon};
use crate::ui::pictures;
use crate::ui::theme;
use crate::ui::widgets::{
    bottom_bar, icon_button, icon_button_at, secondary_button, shortcut, toggle_icon_button,
    top_bar, truncated_text, whole_step_screen,
};
use iced::widget::{
    Column, Stack, button, center, column, container, image, row, scrollable, space, text,
    text_input, toggler, tooltip,
};
use iced::{Alignment, ContentFit, Element, Fill};
use nubo_frontend::cheats;
use nubo_frontend::controls::Action;
use nubo_frontend::save_states::{self, Slot, SlotInfo};
use nubo_frontend::screenshots;

const MENU_WIDTH: f32 = 400.0;
const MENU_MAX_HEIGHT: f32 = 560.0;
const SLOT_THUMBNAIL_WIDTH: f32 = 80.0;
const SLOT_THUMBNAIL_HEIGHT: f32 = 72.0;

pub fn view<'a>(game: &'a Game, settings: &'a Settings, fullscreen: bool) -> Element<'a, Message> {
    let screen = game.session.screen();
    let picture: Element<'a, Message> =
        match screen.map(|screen| (pictures::handle(screen), screen)) {
            Some((handle, screen)) if settings.integer_scaling => {
                whole_step_screen(handle, (screen.width(), screen.height()))
            }
            Some((handle, _)) => image(handle)
                .filter_method(image::FilterMethod::Nearest)
                .content_fit(ContentFit::Contain)
                .width(Fill)
                .height(Fill)
                .into(),
            None => space().into(),
        };

    let mut layers: Vec<Element<'a, Message>> = vec![
        container(picture)
            .center(Fill)
            .padding(12)
            .style(theme::screen_area)
            .into(),
    ];

    if game.session.is_fast_forwarding() {
        layers.push(fast_forward_badge(game, settings));
    }

    if let Some(page) = game.menu() {
        layers.push(pause_menu(game, page, settings, fullscreen));
    }

    if !fullscreen {
        return column![bar(game, settings, false), Stack::with_children(layers)].into();
    }

    if game.controls_visible() {
        layers.push(
            container(bar(game, settings, true))
                .align_bottom(Fill)
                .into(),
        );
    }

    Stack::with_children(layers).into()
}

/// The game screen alone, filling the window, for `--scale` (see
/// `command_line`): nothing is drawn over it, and the window is sized to show
/// it at a whole multiple of its size (see `app::screen_only_size`), with a
/// Super Game Boy's border unless the settings hide it. Black until the game
/// shows its first frame.
pub fn screen_only(game: Option<&Game>) -> Element<'_, Message> {
    match game
        .and_then(|game| game.session.screen())
        .map(pictures::handle)
    {
        Some(handle) => image(handle)
            .filter_method(image::FilterMethod::Nearest)
            .content_fit(ContentFit::Fill)
            .width(Fill)
            .height(Fill)
            .into(),
        None => container(space())
            .width(Fill)
            .height(Fill)
            .style(|_| container::Style::default().background(iced::Color::BLACK))
            .into(),
    }
}

/// The bar with the game's name and controls: at the top of the window, or
/// along the bottom of the screen in fullscreen, with its tooltips above the
/// buttons.
fn bar<'a>(game: &'a Game, settings: &'a Settings, fullscreen: bool) -> Element<'a, Message> {
    let tooltips = if fullscreen {
        tooltip::Position::Top
    } else {
        tooltip::Position::Bottom
    };
    let button =
        |icon, label: String, message| icon_button_at(icon, label, Some(message), tooltips);

    let (pause_icon, pause_label) = if game.is_paused() {
        (Icon::Play, "Resume (Esc)")
    } else {
        (Icon::Pause, "Pause (Esc)")
    };
    let (volume_icon, volume_label) = if settings.shared.muted {
        (Icon::VolumeX, "Unmute")
    } else {
        (Icon::Volume2, "Mute")
    };
    let (fullscreen_icon, fullscreen_label) = if fullscreen {
        (Icon::Minimize, "Exit fullscreen (F11)")
    } else {
        (Icon::Maximize, "Fullscreen (F11)")
    };

    let mut controls = row![
        button(
            Icon::House,
            String::from("Library"),
            Message::Game(GameMessage::CloseGame)
        ),
        truncated_text(game.session.name(), 15.0, theme::HEADING_FONT),
        toggle_icon_button(
            Icon::FastForward,
            String::from("Fast forward (hold Tab)"),
            game.session.is_fast_forward_on(),
            Message::Game(GameMessage::ToggleFastForward),
            tooltips,
        ),
        button(
            volume_icon,
            String::from(volume_label),
            Message::Settings(SettingsMessage::ToggleMute)
        ),
        button(
            pause_icon,
            String::from(pause_label),
            Message::Game(GameMessage::TogglePause)
        ),
        button(
            Icon::RotateCcw,
            format!("Reset ({})", shortcut("R")),
            Message::Game(GameMessage::Reset)
        ),
    ];

    if screenshots::AVAILABLE && game.keeps_data {
        controls = controls.push(button(
            Icon::Camera,
            String::from("Screenshot (F12)"),
            Message::Game(GameMessage::Screenshot),
        ));
    }

    let controls = controls.extend([
        button(
            fullscreen_icon,
            String::from(fullscreen_label),
            Message::Window(WindowMessage::ToggleFullscreen),
        ),
        button(
            Icon::Settings,
            format!("Settings ({})", shortcut(",")),
            Message::Settings(SettingsMessage::Open),
        ),
    ]);

    if fullscreen {
        bottom_bar(controls)
    } else {
        top_bar(controls)
    }
}

/// The label in the top-right corner while fast forwarding.
fn fast_forward_badge<'a>(game: &Game, settings: &Settings) -> Element<'a, Message> {
    let badge = container(
        row![
            icon(Icon::FastForward).size(14),
            text(
                game.session
                    .fast_forward_speed(&settings.shared)
                    .to_string()
            )
            .size(13),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([4, 10])
    .style(theme::floating_card);

    container(badge).align_right(Fill).padding(16).into()
}

/// A row of the pause menu's main page.
pub struct MenuEntry {
    icon: Icon,
    label: &'static str,
    hint: Option<String>,
    pub message: Message,
}

/// The rows of the pause menu's main page, in order. The D-pad moves through
/// them and A picks one, so the view and the app both use this list.
/// `fullscreen` says whether the window fills the screen; hints show the
/// keys bound in `settings`. Save states and cheats are left out for a game
/// that keeps neither (`--fresh`).
pub fn main_entries(game: &Game, settings: &Settings, fullscreen: bool) -> Vec<MenuEntry> {
    let entry = |icon, label, hint: Option<String>, message| MenuEntry {
        icon,
        label,
        hint,
        message,
    };
    let key = |action, what: &str| {
        settings
            .shared
            .controls
            .keyboard
            .input(action)
            .map(|key| format!("{key} {what}").trim().to_owned())
    };

    let mut entries = vec![entry(
        Icon::Play,
        "Resume",
        Some("Esc".into()),
        Message::Game(GameMessage::TogglePause),
    )];

    if save_states::AVAILABLE && game.keeps_data {
        entries.push(entry(
            Icon::Save,
            "Save state",
            key(Action::QuickSave, "quick save"),
            Message::Game(GameMessage::ShowPauseMenu(PauseMenu::Save)),
        ));
        entries.push(entry(
            Icon::History,
            "Load state",
            key(Action::QuickLoad, "quick load"),
            Message::Game(GameMessage::ShowPauseMenu(PauseMenu::Load)),
        ));
    }

    if cheats::AVAILABLE && game.keeps_data {
        entries.push(entry(
            Icon::WandSparkles,
            "Cheats",
            None,
            Message::Game(GameMessage::ShowPauseMenu(PauseMenu::Cheats)),
        ));
    }

    let (fullscreen_icon, fullscreen_label) = if fullscreen {
        (Icon::Minimize, "Exit fullscreen")
    } else {
        (Icon::Maximize, "Fullscreen")
    };
    entries.push(entry(
        fullscreen_icon,
        fullscreen_label,
        key(Action::Fullscreen, ""),
        Message::Window(WindowMessage::ToggleFullscreen),
    ));

    entries.extend([
        entry(
            Icon::RotateCcw,
            "Reset",
            Some(shortcut("R")),
            Message::Game(GameMessage::Reset),
        ),
        entry(
            Icon::Settings,
            "Settings",
            Some(shortcut(",")),
            Message::Settings(SettingsMessage::Open),
        ),
        entry(
            Icon::House,
            "Back to library",
            None,
            Message::Game(GameMessage::CloseGame),
        ),
    ]);

    entries
}

/// The slots listed on the save or load page. Loading doesn't list the
/// "where you left off" slot while it's empty.
pub fn listed_slots(game: &Game, saving: bool) -> Vec<&SlotInfo> {
    game.session
        .slots()
        .iter()
        .filter(|info| saving || info.slot != Slot::Resume || info.saved_at.is_some())
        .collect()
}

/// What picking a slot does: save to it, or load it if it isn't empty.
pub fn slot_message(info: &SlotInfo, saving: bool) -> Option<Message> {
    if saving {
        Some(Message::Game(GameMessage::SaveState(info.slot)))
    } else {
        info.saved_at
            .map(|_| Message::Game(GameMessage::LoadState(info.slot)))
    }
}

/// The pause menu over the dimmed game.
fn pause_menu<'a>(
    game: &'a Game,
    page: PauseMenu,
    settings: &'a Settings,
    fullscreen: bool,
) -> Element<'a, Message> {
    let selection = game.menu_selection();
    let content = match page {
        PauseMenu::Main => main_page(game, selection, settings, fullscreen),
        PauseMenu::Save => slots_page("Save state", listed_slots(game, true), true, selection),
        PauseMenu::Load => slots_page("Load state", listed_slots(game, false), false, selection),
        PauseMenu::Cheats => cheats_page(game, selection),
    };

    center(
        container(scrollable(content).spacing(12))
            .padding(20)
            .width(MENU_WIDTH)
            .max_height(MENU_MAX_HEIGHT)
            .style(theme::panel),
    )
    .style(theme::backdrop)
    .into()
}

fn main_page<'a>(
    game: &'a Game,
    selection: usize,
    settings: &Settings,
    fullscreen: bool,
) -> Element<'a, Message> {
    let rows = main_entries(game, settings, fullscreen)
        .into_iter()
        .enumerate()
        .map(|(index, entry)| menu_item(entry, index == selection));

    column![
        text(game.session.name()).size(18).font(theme::HEADING_FONT),
        text("Paused").size(13).style(theme::muted_text),
        space().height(8),
        Column::with_children(rows).spacing(2),
    ]
    .spacing(2)
    .into()
}

/// A row of the pause menu: an icon, a label and a hint such as a shortcut.
/// The row the D-pad is on is highlighted.
fn menu_item<'a>(entry: MenuEntry, selected: bool) -> Element<'a, Message> {
    let mut content = row![icon(entry.icon), text(entry.label).width(Fill)]
        .spacing(12)
        .align_y(Alignment::Center);

    if let Some(hint) = entry.hint {
        content = content.push(text(hint).size(12).style(theme::muted_text));
    }

    button(content)
        .padding([10, 12])
        .width(Fill)
        .style(theme::segment(selected))
        .on_press(entry.message)
        .into()
}

/// The title of a page below the main one, with a button back to it.
fn page_header(title: &str) -> Element<'_, Message> {
    row![
        icon_button(
            Icon::ArrowLeft,
            String::from("Back (Esc)"),
            Some(Message::Game(GameMessage::ShowPauseMenu(PauseMenu::Main))),
        ),
        text(title).size(18).font(theme::HEADING_FONT),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

/// The save or load page: one row per slot with its picture and age.
fn slots_page<'a>(
    title: &'a str,
    slots: Vec<&'a SlotInfo>,
    saving: bool,
    selection: usize,
) -> Element<'a, Message> {
    let rows = slots
        .into_iter()
        .enumerate()
        .map(|(index, info)| slot_row(info, saving, index == selection));

    column![page_header(title), Column::with_children(rows).spacing(6)]
        .spacing(12)
        .into()
}

fn slot_row(info: &SlotInfo, saving: bool, selected: bool) -> Element<'_, Message> {
    let picture: Element<'_, Message> = match info.thumbnail.as_ref().map(pictures::handle) {
        Some(handle) => image(handle)
            .filter_method(image::FilterMethod::Nearest)
            .width(SLOT_THUMBNAIL_WIDTH)
            .height(SLOT_THUMBNAIL_HEIGHT)
            .border_radius(4)
            .into(),
        None => center(icon(Icon::Gamepad2).size(22).style(theme::muted_text))
            .width(SLOT_THUMBNAIL_WIDTH)
            .height(SLOT_THUMBNAIL_HEIGHT)
            .style(theme::thumbnail_placeholder)
            .into(),
    };

    let age = info
        .saved_at
        .map_or_else(|| String::from("Empty"), save_states::time_ago);

    button(
        row![
            picture,
            column![
                text(info.slot.to_string()).size(15),
                text(age).size(12).style(theme::muted_text),
            ]
            .spacing(4),
        ]
        .spacing(14)
        .align_y(Alignment::Center),
    )
    .padding(8)
    .width(Fill)
    .style(theme::selectable_card(selected))
    .on_press_maybe(slot_message(info, saving))
    .into()
}

/// The cheats page: the game's cheats with a switch each, and a form to add
/// one.
fn cheats_page(game: &Game, selection: usize) -> Element<'_, Message> {
    let cheats = game.session.cheats();
    let list: Element<'_, Message> = if cheats.is_empty() {
        text("No cheats yet. Add a GameShark or Game Genie code below.")
            .size(13)
            .style(theme::muted_text)
            .into()
    } else {
        Column::with_children(cheats.iter().enumerate().map(|(index, cheat)| {
            container(
                row![
                    toggler(cheat.enabled)
                        .size(18)
                        .on_toggle(move |enabled| Message::Game(GameMessage::CheatToggled(
                            index, enabled
                        ))),
                    column![
                        text(&cheat.name).size(14),
                        text(&cheat.code).size(12).style(theme::muted_text),
                    ]
                    .spacing(2)
                    .width(Fill),
                    icon_button(
                        Icon::Trash2,
                        String::from("Remove cheat"),
                        Some(Message::Game(GameMessage::RemoveCheat(index))),
                    ),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            )
            .padding([6, 10])
            .style(theme::selectable_row(index == selection))
            .into()
        }))
        .spacing(6)
        .into()
    };

    let form = &game.cheat_form;
    let mut add = column![
        text_input("Name (optional)", &form.name)
            .on_input(|name| Message::Game(GameMessage::CheatNameChanged(name)))
            .padding([7, 10])
            .style(theme::search_field),
        row![
            text_input("Code, such as 01FF56D0 or 00A-17B-C49", &form.code)
                .on_input(|code| Message::Game(GameMessage::CheatCodeChanged(code)))
                .on_submit(Message::Game(GameMessage::AddCheat))
                .padding([7, 10])
                .style(theme::search_field),
            secondary_button(Icon::Plus, "Add", Message::Game(GameMessage::AddCheat)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(8);

    if let Some(error) = &form.error {
        add = add.push(
            text(error)
                .size(12)
                .style(|theme: &iced::Theme| text::Style {
                    color: Some(theme.palette().danger),
                }),
        );
    }

    column![
        page_header("Cheats"),
        list,
        space().height(4),
        text("Add a cheat").size(13).style(theme::muted_text),
        add,
    ]
    .spacing(12)
    .into()
}
