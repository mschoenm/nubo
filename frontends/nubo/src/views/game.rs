//! The game screen: a bar with the game's controls and the Game Boy screen,
//! and the pause menu with its save state, cheats and Super Game Boy
//! (`sgb.rs`) pages. The bar keeps only what a player reaches for while
//! playing: the way back to the library on the left, and fast forward,
//! sound with its volume, fullscreen and Pause on the right. Pause opens the
//! pause menu, which holds everything else. In a window the bar sits above
//! the picture. In fullscreen the picture fills the screen and the bar
//! appears over its top when the mouse moves. Double-clicking the picture
//! switches between fullscreen and a window, as on a video site. The pause
//! menu works like a context menu: a right click on the picture opens it, as
//! Esc does, and a click on the game outside it or a row that leaves it
//! behind closes it. The bar's buttons work as usual while it's open. With
//! `--scale` the window shows the picture alone ([`screen_only`]).

use crate::app::{GameMessage, Message, SettingsMessage, WindowMessage};
use crate::game::{Game, PauseMenu};
use crate::settings::Settings;
use crate::ui::icons::{Icon, icon};
use crate::ui::pictures;
use crate::ui::theme;
use crate::ui::widgets::{
    bar_button, bar_separator, icon_button, menu_separator, secondary_button, shortcut,
    steady_bar_button, toggle_icon_button, top_bar, truncated_text, whole_step_screen,
};
use iced::widget::{
    self, Column, Stack, button, center, column, container, image, mouse_area, opaque, row,
    scrollable, slider, space, text, text_input, toggler,
};
use iced::{Alignment, ContentFit, Element, Fill, mouse};
use nubo_frontend::cheats;
use nubo_frontend::controls::Action;
use nubo_frontend::library;
use nubo_frontend::save_states::{self, Slot, SlotInfo};
use nubo_frontend::screenshots;

const MENU_WIDTH: f32 = 400.0;
/// The width of the volume slider in the bar.
const VOLUME_SLIDER_WIDTH: f32 = 80.0;
const MENU_MAX_HEIGHT: f32 = 560.0;
const SLOT_THUMBNAIL_WIDTH: f32 = 80.0;
const SLOT_THUMBNAIL_HEIGHT: f32 = 72.0;
/// The wider of the Pause button's two labels, which sets its width so the
/// bar doesn't move when the game pauses or resumes.
const PAUSE_WIDEST: &str = "Resume";

/// The game screen: the Game Boy screen, the fast forward or rewinding
/// badge, the pause menu when it's open, and the bar, above the picture in a
/// window and over its top in fullscreen while the mouse moves.
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
        mouse_area(
            container(picture)
                .center(Fill)
                .padding(12)
                .style(move |theme| theme::screen_area(theme, fullscreen)),
        )
        .on_double_click(Message::Window(WindowMessage::ToggleFullscreen))
        .on_right_press(Message::Game(GameMessage::OpenPauseMenu))
        .interaction(if game.pointer_hidden() {
            mouse::Interaction::Hidden
        } else {
            mouse::Interaction::None
        })
        .into(),
    ];

    if game.session.is_rewinding() && !game.is_paused() {
        layers.push(corner_badge(Icon::Rewind, String::from("Rewinding")));
    } else if game.session.is_fast_forwarding() {
        layers.push(fast_forward_badge(game, settings));
    }

    if let Some(page) = game.menu() {
        layers.push(pause_menu(game, page, settings, fullscreen));
    }

    if !fullscreen {
        return column![bar(game, settings, false), Stack::with_children(layers)].into();
    }

    if game.controls_visible() {
        layers.push(container(bar(game, settings, true)).align_top(Fill).into());
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

/// The bar with the game's name and controls, at the top of the window, or
/// over the top of the picture in fullscreen. The two buttons a player has
/// to find, back to the library and Pause, carry their names.
fn bar<'a>(game: &'a Game, settings: &'a Settings, fullscreen: bool) -> Element<'a, Message> {
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
    // The browser has no library: leaving a game goes back to the start
    // screen.
    let (back_label, back_tip) = if library::AVAILABLE {
        ("Library", "Back to the library")
    } else {
        ("Home", "Back to the start screen")
    };
    let pause = if game.is_paused() {
        steady_bar_button(
            Icon::Play,
            ("Resume", PAUSE_WIDEST),
            String::from("Resume (Esc)"),
            true,
            Message::Game(GameMessage::TogglePause),
        )
    } else {
        steady_bar_button(
            Icon::Pause,
            ("Pause", PAUSE_WIDEST),
            String::from("Pause and open the menu (Esc)"),
            false,
            Message::Game(GameMessage::TogglePause),
        )
    };

    top_bar(row![
        bar_button(
            Icon::ArrowLeft,
            back_label,
            String::from(back_tip),
            false,
            Message::Game(GameMessage::CloseGame),
        ),
        space().width(4),
        bar_separator(),
        space().width(4),
        truncated_text(game.session.name(), 15.0, theme::HEADING_FONT),
        toggle_icon_button(
            Icon::FastForward,
            String::from("Fast forward (hold Tab)"),
            game.session.is_fast_forward_on(),
            Message::Game(GameMessage::ToggleFastForward),
        ),
        icon_button(
            volume_icon,
            String::from(volume_label),
            Some(Message::Settings(SettingsMessage::ToggleMute)),
        ),
        slider(
            0.0..=1.0_f32,
            settings.shared.effective_volume(),
            |volume| Message::Settings(SettingsMessage::VolumeChanged(volume)),
        )
        .step(0.01_f32)
        .on_release(Message::Settings(SettingsMessage::VolumeReleased))
        .width(VOLUME_SLIDER_WIDTH),
        space().width(4),
        icon_button(
            fullscreen_icon,
            String::from(fullscreen_label),
            Some(Message::Window(WindowMessage::ToggleFullscreen)),
        ),
        space().width(6),
        pause,
    ])
}

/// The label in the top-right corner while fast forwarding.
fn fast_forward_badge<'a>(game: &Game, settings: &Settings) -> Element<'a, Message> {
    corner_badge(
        Icon::FastForward,
        game.session
            .fast_forward_speed(&settings.shared)
            .to_string(),
    )
}

/// A small label in the top-right corner of the picture, such as while
/// fast forwarding or rewinding.
fn corner_badge<'a>(symbol: Icon, label: String) -> Element<'a, Message> {
    let badge = container(
        row![icon(symbol).size(14), text(label).size(13)]
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
    /// Whether a thin line above the row starts a new group of rows.
    starts_group: bool,
    pub message: Message,
}

/// The rows of the pause menu's main page, in order. The D-pad moves through
/// them and A picks one, so the view and the app both use this list.
/// `fullscreen` says whether the window fills the screen; hints show the
/// keys bound in `settings`. Save states, cheats and screenshots are left
/// out for a game that keeps none of them (`--fresh`). The rows come in
/// groups: going on playing, the game's saves and extras, the picture and
/// the app, and leaving the game.
pub fn main_entries(game: &Game, settings: &Settings, fullscreen: bool) -> Vec<MenuEntry> {
    let entry = |icon, label, hint: Option<String>, message| MenuEntry {
        icon,
        label,
        hint,
        starts_group: false,
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

    let playing = vec![entry(
        Icon::Play,
        "Resume",
        Some("Esc".into()),
        Message::Game(GameMessage::TogglePause),
    )];

    let mut extras = Vec::new();
    if save_states::AVAILABLE && game.keeps_data {
        extras.push(entry(
            Icon::Save,
            "Save state",
            key(Action::QuickSave, "quick save"),
            Message::Game(GameMessage::ShowPauseMenu(PauseMenu::Save)),
        ));
        extras.push(entry(
            Icon::History,
            "Load state",
            key(Action::QuickLoad, "quick load"),
            Message::Game(GameMessage::ShowPauseMenu(PauseMenu::Load)),
        ));
    }
    if cheats::AVAILABLE && game.keeps_data {
        extras.push(entry(
            Icon::WandSparkles,
            "Cheats",
            None,
            Message::Game(GameMessage::ShowPauseMenu(PauseMenu::Cheats)),
        ));
    }
    if game.session.model().is_sgb() {
        extras.push(entry(
            Icon::Palette,
            "Super Game Boy",
            None,
            Message::Game(GameMessage::ShowPauseMenu(PauseMenu::SuperGameBoy)),
        ));
    }

    let mut app = Vec::new();
    if screenshots::AVAILABLE && game.keeps_data {
        app.push(entry(
            Icon::Camera,
            "Screenshot",
            key(Action::Screenshot, ""),
            closing_menu(Message::Game(GameMessage::Screenshot)),
        ));
    }
    let (fullscreen_icon, fullscreen_label) = if fullscreen {
        (Icon::Minimize, "Exit fullscreen")
    } else {
        (Icon::Maximize, "Fullscreen")
    };
    app.extend([
        entry(
            fullscreen_icon,
            fullscreen_label,
            key(Action::Fullscreen, ""),
            closing_menu(Message::Window(WindowMessage::ToggleFullscreen)),
        ),
        entry(
            Icon::Settings,
            "Settings",
            Some(shortcut(",")),
            closing_menu(Message::Settings(SettingsMessage::Open)),
        ),
    ]);

    let leaving = vec![
        entry(
            Icon::RotateCcw,
            "Restart game",
            Some(shortcut("R")),
            Message::Game(GameMessage::Reset),
        ),
        entry(
            Icon::ArrowLeft,
            // The browser has no library, as in the bar.
            if library::AVAILABLE {
                "Back to library"
            } else {
                "Back to the start screen"
            },
            None,
            Message::Game(GameMessage::CloseGame),
        ),
    ];

    let mut entries = Vec::new();
    for mut group in [playing, extras, app, leaving] {
        if let Some(first) = group.first_mut() {
            first.starts_group = !entries.is_empty();
        }
        entries.append(&mut group);
    }

    entries
}

/// `message`, after closing the pause menu, for a row that leaves the menu
/// behind as a context menu does. The rows that resume the game, start it
/// again or leave it close the menu by themselves, and the rows that open
/// another page keep it open.
fn closing_menu(message: Message) -> Message {
    Message::Game(GameMessage::PickedFromMenu(Box::new(message)))
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

/// The pause menu over the dimmed game. A click on the game outside the
/// menu's panel, with either button, closes it, as it does a context menu;
/// the bar isn't covered, so its buttons do what they always do.
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
        PauseMenu::SuperGameBoy => sgb::page(game, settings, selection),
    };

    // Clicks anywhere on the panel, even between its rows, stay in it.
    let panel = opaque(
        container(scrollable(content).id(pause_scroll_id()).spacing(12))
            .padding(20)
            .width(MENU_WIDTH)
            .max_height(MENU_MAX_HEIGHT)
            .style(theme::panel),
    );

    // The Super Game Boy page sits at the side over the undimmed game, so
    // the player sees the colors and border they pick.
    let layer = if page == PauseMenu::SuperGameBoy {
        container(panel)
            .align_right(Fill)
            .center_y(Fill)
            .padding(16)
    } else {
        center(panel).style(theme::backdrop)
    };

    let close = Message::Game(GameMessage::ClosePauseMenu);
    mouse_area(layer)
        .on_press(close.clone())
        .on_right_press(close)
        .into()
}

/// The pause menu's first page: the game's name and the menu's rows, in
/// groups, with the row the D-pad is on highlighted.
fn main_page<'a>(
    game: &'a Game,
    selection: usize,
    settings: &Settings,
    fullscreen: bool,
) -> Element<'a, Message> {
    let mut rows = Column::new().spacing(2);
    for (index, entry) in main_entries(game, settings, fullscreen)
        .into_iter()
        .enumerate()
    {
        if entry.starts_group {
            rows = rows.push(menu_separator());
        }
        rows = rows.push(menu_item(entry, index == selection));
    }

    column![
        text(game.session.name()).size(18).font(theme::HEADING_FONT),
        text("Paused").size(13).style(theme::muted_text),
        space().height(8),
        rows,
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

/// The pause menu's scrolling page, so the app can scroll it to the row the
/// D-pad is on.
pub fn pause_scroll_id() -> widget::Id {
    widget::Id::new("pause-menu")
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
    let explanation = if saving {
        "Keeps this exact moment, to come back to later. The game's own saves are kept \
         separately."
    } else {
        "Goes back to a moment you kept. You can undo it right after."
    };

    column![
        page_header(title),
        text(explanation).size(12).style(theme::muted_text),
        Column::with_children(rows).spacing(6)
    ]
    .spacing(12)
    .into()
}

/// A save state slot as a row of the Save or Load page: its picture (or a
/// placeholder), its name and how long ago it was saved. An empty slot can't
/// be picked for loading.
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
        text_input("Code", &form.code)
            .on_input(|code| Message::Game(GameMessage::CheatCodeChanged(code)))
            .on_submit(Message::Game(GameMessage::AddCheat))
            .padding([7, 10])
            .style(theme::search_field),
        text(
            "Such as 01FF56D0 (GameShark) or 00A-17B-C49 (Game Genie). For a cheat with \
             several codes, put a space or + between them."
        )
        .size(12)
        .style(theme::muted_text),
        row![
            text_input("Name (optional)", &form.name)
                .on_input(|name| Message::Game(GameMessage::CheatNameChanged(name)))
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

pub mod sgb;
