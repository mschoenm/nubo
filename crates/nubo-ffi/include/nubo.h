/*
 * nubo.h: Nubo for apps written in other languages, such as the iOS app in
 * Swift. It exposes nubo-frontend (everything an app needs that isn't
 * drawing) as plain C functions, written by hand in crates/nubo-ffi/src/ffi.rs.
 * docs/IOS_PLAN.md has the plan this header belongs to.
 *
 * Copyright 2026 Michel Schoenmakers. Part of Nubo, GPL-3.0-or-later (see
 * LICENSE and NOTICE).
 *
 * Rules for every function:
 *
 * - Strings going in are UTF-8 and end with a NUL byte. Strings coming out
 *   (`char *`) belong to the caller, who frees them with nubo_string_free.
 *   `const char *` coming out belongs to Nubo; how long it stays valid is
 *   written on the function.
 * - Functions that can fail return false (or NULL) and, when `error` isn't
 *   NULL, set `*error` to a message the app can show the player.
 * - Objects (NuboSettings, NuboLibrary, NuboGame, NuboGameList) are opaque.
 *   Each is freed exactly once with its own _free function; passing NULL to
 *   a _free function does nothing.
 * - Every function may be called from any thread. Each object locks itself.
 * - No function panics into C: a panic inside Nubo is caught, logged, and
 *   the call fails as above.
 */

#ifndef NUBO_H
#define NUBO_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ---- Versions ---------------------------------------------------------- */

/* The version of this header. A Nubo built for another one refuses it:
 * check nubo_api_version() == NUBO_API_VERSION at startup. */
#define NUBO_API_VERSION 1

/* The header version Nubo was built with. */
uint32_t nubo_api_version(void);

/* Nubo's version and build, such as "0.12.0 (7c44d39 2026-10-16)", valid
 * for the whole run. */
const char *nubo_version(void);

/* ---- Strings ----------------------------------------------------------- */

/* Frees a string Nubo handed over. NULL does nothing. */
void nubo_string_free(char *string);

/* ---- Where data is kept ------------------------------------------------ */

/* Keeps every file in `folder` for the rest of the run: the settings in
 * folder/config and the rest (battery saves, save states, cheats, the
 * library) in folder/data, in the same formats as the desktop app. Call it
 * once, before anything else, with the app's Application Support folder.
 * Only the first call counts; it returns whether it was that one. */
bool nubo_set_data_folder(const char *folder);

/* ---- Game Boy types ---------------------------------------------------- */

/* A Game Boy button. */
typedef enum NuboButton {
    NUBO_BUTTON_UP = 0,
    NUBO_BUTTON_DOWN = 1,
    NUBO_BUTTON_LEFT = 2,
    NUBO_BUTTON_RIGHT = 3,
    NUBO_BUTTON_A = 4,
    NUBO_BUTTON_B = 5,
    NUBO_BUTTON_START = 6,
    NUBO_BUTTON_SELECT = 7,
} NuboButton;

/* The hardware a game runs on. AUTOMATIC picks the newest console the
 * cartridge supports. The numbers are kept forever: apps may store them. */
typedef enum NuboModel {
    NUBO_MODEL_AUTOMATIC = 0,
    NUBO_MODEL_DMG0 = 1,  /* The first Game Boy CPU revision */
    NUBO_MODEL_DMG = 2,   /* The original Game Boy */
    NUBO_MODEL_MGB = 3,   /* Game Boy Pocket */
    NUBO_MODEL_SGB = 4,   /* Super Game Boy */
    NUBO_MODEL_SGB2 = 5,  /* Super Game Boy 2 */
    NUBO_MODEL_CGB0 = 6,  /* Game Boy Color, first CPU revision */
    NUBO_MODEL_CGBA = 7,
    NUBO_MODEL_CGBB = 8,
    NUBO_MODEL_CGBC = 9,
    NUBO_MODEL_CGBD = 10,
    NUBO_MODEL_CGBE = 11,
    NUBO_MODEL_AGB = 12,  /* Game Boy Advance, playing Game Boy Color games */
} NuboModel;

/* A place to keep one save state of a game. */
typedef enum NuboSlot {
    NUBO_SLOT_RESUME = 0, /* Where the player left off; load only */
    NUBO_SLOT_QUICK = 1,
    NUBO_SLOT_1 = 2,
    NUBO_SLOT_2 = 3,
    NUBO_SLOT_3 = 4,
} NuboSlot;

/* ---- Pictures ---------------------------------------------------------- */

/* The largest picture Nubo hands over: a Super Game Boy's 256x224 screen with
 * its border, four bytes (RGBA) a pixel. A buffer this big holds any of
 * them. A Game Boy screen alone is 160x144. */
#define NUBO_PICTURE_MAX_WIDTH 256
#define NUBO_PICTURE_MAX_HEIGHT 224
#define NUBO_PICTURE_MAX_BYTES (NUBO_PICTURE_MAX_WIDTH * NUBO_PICTURE_MAX_HEIGHT * 4)

/* Where a picture is copied to: the caller sets `rgba` and `capacity`, Nubo
 * fills `width` and `height` and that many pixels, row by row, with no
 * padding between rows. */
typedef struct NuboPicture {
    uint8_t *rgba;
    size_t capacity;
    uint32_t width;
    uint32_t height;
} NuboPicture;

/* ---- Settings ---------------------------------------------------------- */

typedef struct NuboSettings NuboSettings;

/* Reads the kept settings, or the defaults when none were kept yet. */
NuboSettings *nubo_settings_load(void);
void nubo_settings_free(NuboSettings *settings);

/* Writes the settings to settings.toml. */
bool nubo_settings_save(const NuboSettings *settings, char **error);

/* The volume, from 0 (silent) to 1 (as loud as the game). */
float nubo_settings_volume(const NuboSettings *settings);
void nubo_settings_set_volume(NuboSettings *settings, float volume);

/* The model games start on unless the library says otherwise. */
NuboModel nubo_settings_model(const NuboSettings *settings);
void nubo_settings_set_model(NuboSettings *settings, NuboModel model);

/* Whether games continue where the player left off. */
bool nubo_settings_resume_games(const NuboSettings *settings);
void nubo_settings_set_resume_games(NuboSettings *settings, bool resume);

/* Later: the palette, the screen effects, fast forward's speed, rewind and
 * the Super Game Boy border, each as a getter and a setter like these. */

/* ---- The library ------------------------------------------------------- */

typedef struct NuboLibrary NuboLibrary;
typedef struct NuboGameList NuboGameList;

/* Reads the cached library, with the games kept in `games_folder` (the
 * folder the Files app and the document picker copy games into). */
NuboLibrary *nubo_library_load(const char *games_folder, char **error);
void nubo_library_free(NuboLibrary *library);

/* Looks through the games folder again for games added or removed, and
 * captures the title screens still missing. Slow: it blocks until done, so
 * call it off the main thread. */
bool nubo_library_scan(NuboLibrary *library, char **error);

/* The games whose name matches `search` (all of them for ""), in the
 * library's order, as a list that stays as it is until freed. */
NuboGameList *nubo_library_games(const NuboLibrary *library, const char *search);

/* The `limit` games played most recently, newest first. */
NuboGameList *nubo_library_recently_played(const NuboLibrary *library, size_t limit);

/* The model the game at `path` starts on: the one picked for it in the
 * library, else the one `settings` pick, for its cartridge. AUTOMATIC if
 * its file can't be read. */
NuboModel nubo_library_start_model(const NuboLibrary *library, const NuboSettings *settings,
                                   const char *path);

/* The model where the player left off in the game at `path` was saved on,
 * which it can only continue on; AUTOMATIC if there is nothing to continue. */
NuboModel nubo_library_resume_model(const NuboLibrary *library, const char *path);

/* Copies the game's title screen, or its last screen played, into
 * `picture`. False if none was captured yet or `picture` is too small. */
bool nubo_library_picture(const NuboLibrary *library, const char *path, NuboPicture *picture);

/* The games in a list. The strings stay valid until the list is freed. */
size_t nubo_game_list_count(const NuboGameList *list);
const char *nubo_game_list_path(const NuboGameList *list, size_t index);
const char *nubo_game_list_name(const NuboGameList *list, size_t index);
/* The model the player picked for this game, or AUTOMATIC. */
NuboModel nubo_game_list_model(const NuboGameList *list, size_t index);
/* When the game was last played, in seconds since 1970, or 0 if never. */
int64_t nubo_game_list_last_played(const NuboGameList *list, size_t index);
void nubo_game_list_free(NuboGameList *list);

/* ---- A running game ---------------------------------------------------- */

typedef struct NuboGame NuboGame;

/* Called whenever the emulator finishes a new frame, on the emulator's
 * thread and while the frame is locked: it must return at once and never
 * call Nubo. Set a flag that the app's CADisplayLink reads, and copy the
 * picture there with nubo_game_screen. */
typedef void (*NuboFrameCallback)(void *context);

/* Starts the game at `path` on `model` (not AUTOMATIC), continuing where
 * the player left off when the settings say so and that was on `model`. The
 * game plays its sound at once; its emulator runs on a thread of its own,
 * paced by the sound card. Before starting, the app asks
 * nubo_library_start_model which model to pass, and, when
 * nubo_library_resume_model names another one, asks the player which to
 * play on, as the desktop app does. */
NuboGame *nubo_game_start(const char *path, NuboModel model, const NuboSettings *settings,
                          char **error);

/* Stops the game and writes its battery saves still waiting (and, when the
 * settings say so, where the player left off). */
void nubo_game_free(NuboGame *game);

/* The model the game runs on (never AUTOMATIC). */
NuboModel nubo_game_model(const NuboGame *game);

void nubo_game_press(NuboGame *game, NuboButton button);
void nubo_game_release(NuboGame *game, NuboButton button);

/* Pauses or resumes, such as while the player is in a menu. */
void nubo_game_set_running(NuboGame *game, bool running);

/* Pauses the game and writes everything still waiting: its battery saves,
 * which are otherwise written up to a second late, and, when the settings
 * say so, where the player left off. Call it when the app goes to the
 * background, since iOS may end a suspended app without a word. */
bool nubo_game_suspend(NuboGame *game, char **error);

void nubo_game_hold_fast_forward(NuboGame *game, bool held);
void nubo_game_hold_rewind(NuboGame *game, bool held);

/* Follows `settings` from now on (volume, palette, screen effects, fast
 * forward's speed), such as after the player changed them. */
void nubo_game_apply_settings(NuboGame *game, const NuboSettings *settings);

/* Calls `callback` with `context` for every new frame from now on, once
 * straight away, replacing any earlier callback. NULL stops the calls. */
void nubo_game_observe_frames(NuboGame *game, NuboFrameCallback callback, void *context);

/* Copies the newest picture, in the chosen palette, with a Super Game Boy's
 * border unless it is off, into `picture`. False if there is none yet or
 * `picture` is too small. */
bool nubo_game_screen(NuboGame *game, NuboPicture *picture);

/* Saves the game's state in `slot` (not RESUME), with the screen as its
 * picture. */
bool nubo_game_save_state(NuboGame *game, NuboSlot slot, char **error);
/* Loads the state in `slot`. */
bool nubo_game_load_state(NuboGame *game, NuboSlot slot, char **error);
/* When `slot` was saved, in seconds since 1970, or 0 if it is empty. */
int64_t nubo_game_slot_saved_at(const NuboGame *game, NuboSlot slot);
/* Copies the screen `slot` was saved with into `picture` (160x144). */
bool nubo_game_slot_picture(const NuboGame *game, NuboSlot slot, NuboPicture *picture);

/* The game's cheats, by index from 0. The name and code are copies, since
 * another thread may change the cheats meanwhile; NULL past the end. */
size_t nubo_game_cheat_count(const NuboGame *game);
char *nubo_game_cheat_name(const NuboGame *game, size_t index);
char *nubo_game_cheat_code(const NuboGame *game, size_t index);
bool nubo_game_cheat_enabled(const NuboGame *game, size_t index);
/* Adds a Game Genie or GameShark code (several joined by "+") and turns it
 * on; `name` may be NULL. */
bool nubo_game_add_cheat(NuboGame *game, const char *name, const char *code, char **error);
bool nubo_game_set_cheat_enabled(NuboGame *game, size_t index, bool enabled, char **error);
bool nubo_game_remove_cheat(NuboGame *game, size_t index, char **error);

/* The next problem found while the game runs, such as a battery save that
 * couldn't be written, for the app to show; NULL when there is none. */
char *nubo_game_take_notice(NuboGame *game);

#ifdef __cplusplus
}
#endif

#endif /* NUBO_H */
