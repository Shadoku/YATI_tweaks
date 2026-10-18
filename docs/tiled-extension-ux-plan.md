# Tiled extension plan, part 2: configuration in one dialog, and no reopen

A UX pass over `tiled-extension/yati-godot/` (its first plan: `docs/tiled-extension-plan.md`),
taken up on 2026-10-18 after the extension ran in a real Tiled 1.12. The extension works; what
it lacks is explanation. *YATI: Configure…* is a chain of five blind prompts (a folder dialog, a
text prompt, a folder dialog, a text prompt, a message box) with the rules in the README rather
than on screen, a wrong answer is found out later by another action, and the sync and the scenes
action end by asking the user to reopen the project by hand. This plan replaces the chain with
one dialog with a field per setting, tooltips, and validation before it closes, and has the
extension reopen the project itself.

## 1. What was checked before planning

- **Tiled's `Dialog` class** (https://www.mapeditor.org/docs/scripting/classes/Dialog.html,
  since 1.9; `src/tiled/scriptdialog.{h,cpp}` of v1.12.0): `new Dialog(title)` with
  `addHeading(text, fillRow)`, `addLabel(text, toolTip)`, `addTextInput(label, default,
  toolTip)` (a `QLineEdit`: `text`, `placeholderText`, `textChanged`, `editingFinished`),
  `addFilePicker(label, default, toolTip)` (a `FileEdit`: `fileName` since 1.11, `fileUrl`,
  `isDirectory`, `filter`, `fileUrlChanged`), `addCheckBox(text, default, toolTip)`
  (`checked`, `stateChanged`), `addComboBox(label, values, defaultIndex, toolTip)`
  (`currentIndex`, `currentIndexChanged`), `addTextEdit` (`html`, `readOnly`), `addButton(text,
  toolTip)` (`clicked`), `addSeparator(label)`, `addNewRow()`, `newRowMode`
  (`SameWidgetRows`, `ManualRows`, `SingleWidgetRows`), `exec()` (blocks the script until the
  dialog closes), `show()`, `accept()`, `reject()`, `done()`, `accepted`, `rejected`,
  `finished`. Every `add*` but `addHeading` takes a `toolTip` **since 1.12**, applied to the
  widget and its label (`addDialogWidget`), and every widget inherits `QWidget`'s `toolTip`,
  `enabled`, `visible`, `styleSheet`. The dialog adds **no OK or Cancel button of its own**: the
  script adds buttons and closes the dialog from their handlers, so a handler can refuse to close.
  The Qt widgets have no reference pages (only `Dialog`, `FileEdit`, `ColorButton`,
  `ImageWidget`); their members are declared in `@mapeditor/tiled-api` 1.12.0's `index.d.ts`
  (`QLineEdit`, `QCheckBox`, `QComboBox`, `QPushButton` extends `QAbstractButton`, `QLabel`,
  `QTextEdit`). `QAbstractButton::click()` is a public slot, so a test can press a button.
- **Reopening the project from a script.** Tiled 1.12.2 (the latest, 2026-05-27; 1.12.1 and
  1.12.2 changed nothing in the scripting API) still has no `valuesAsFlags` on
  `EnumPropertyType`, no typed `setMember`, and no reload-project function. But `tiled.open(path)`
  (`ScriptModule::open`) calls `DocumentManager::openFile`, which only emits
  `fileOpenRequested`, and `MainWindow` connects that to `MainWindow::openFile(path)`, which
  begins with `if (fileName.endsWith(".tiled-project")) return openProjectFile(fileName);`.
  `openProjectFile` does `Project::load(fileName)` (from disk) and `switchProject`, which emits
  `Preferences::aboutToSwitchSession` (on which `DocumentManager::updateSession` records the
  open files and the active one in the session), calls `closeAllFiles()` (asking about unsaved
  changes; a cancel aborts the switch), unloads the worlds, switches to the project's session,
  sets the project in `ProjectManager` and calls `restoreSession()`, which reopens the recorded
  files and reactivates the active one. So **`tiled.open(tiled.project.fileName)` is the manual
  reopen, done by the script**: the project is read from disk again, with what the extension
  patched into the file, and the open maps come back. `ScriptModule::open` returns null for it
  (`findDocument` finds no document) and throws when there is no `DocumentManager` (under
  `--evaluate`). `ProjectManager::setProject` refreshes the extension paths; they are the same
  path, so the script engine is not reset under the running action. None of this is in the
  reference: it is read from v1.12.0's `scriptmodule.cpp`, `documentmanager.cpp` and
  `mainwindow.cpp`, and only the editor can confirm it (stage 2's checklist).
- **The current `configure()`** (`yati-godot.mjs`): `promptDirectory` for the Godot project
  (cancel aborts everything), `prompt` for the binary (empty keeps), `promptDirectory` for the
  scenes folder (cancel means the whole project, which is also what a fresh user does by
  mistake), `prompt` for the import options (`-` clears), then one `alert` with the result and
  the option warnings. The project is checked for `project.godot`; nothing else is checked.
  `settings.mjs` already has the pieces a dialog needs: `readSettings` with the raw and the
  resolved value of each setting, `storedPath`, `localToGodot`, `parseImportOptions` with its
  warnings, `IMPORT_OPTIONS` with each option's kind. `Runtime.md` gives the string options'
  values: `parallax_mode` `parallax2d` | `legacy`, `animation_mode` `strip` | `atlas`,
  `object_refs` `nodepath` | `geometry` | `both`, `unknown_class_fallback` a mapping-table class
  or `node2d`; `custom_data_prefix` and `save_tileset_to` are free text.
- **The tests' reach.** The fake (`test/helpers/fake_tiled.mjs`) answers the prompts from
  lists; `api_surface.mjs` fails a test when the glue touches an API member it does not list.
  The real Tiled run (`test/real_tiled.test.mjs`, `tiled --evaluate` under `xvfb-run`) has a
  `QApplication` but no `MainWindow`: a `QDialog` can be built and its widgets set and read,
  `exec()` must not be called, and `tiled.open` throws. The harness stubs the dialogs today.

## 2. Decisions

- **One dialog, `YATI: Configure…`**, built in a new module `lib/configure.mjs` (the field
  list, the tooltip texts, the validation: pure, tested under Node) and shown by the glue with
  `Dialog`. `newRowMode = SingleWidgetRows`, so each field is a row with its label on the left.
  Its rows, top to bottom:
  1. A heading and one line of explanation: the settings are custom properties of the open
     project (`<project file>`), saved with it and shared with everyone who opens it; paths
     are stored relative to it when they can be.
  2. **Godot project** (`godot_project`): a directory `FileEdit`, default the current value.
     Tooltip: the folder that holds `project.godot`; what the other actions read from it; how
     it is stored. A status label under it says `project.godot found: <project name>` (read
     with `readGodotProject`) or what is wrong, updated on `fileUrlChanged`.
  3. **Godot binary** (`godot_binary`): a text input (a bare command such as `godot` must stay
     possible, which a file picker cannot hold) with a *Browse…* button next to it
     (`promptOpenFile`) and a *Test* button that runs `<binary> --version` with `Process` and
     writes the result into the status label (`Godot 4.7.1.stable`, or that it could not
     start). Tooltip: a path or a command on `PATH`, used only by *Check map in Godot*,
     relative paths are relative to the project file. Placeholder `godot`.
  4. **Scenes folder** (`godot_scenes_dir`): a directory `FileEdit`, empty for the whole Godot
     project (the placeholder says so). Tooltip: where *Import scenes as classes* looks for
     `.tscn` files; `addons/` is skipped unless this points into it; stored as `res://…` when
     inside the Godot project.
  5. **Import options** (`yati_import_options`): a text input with the `name=value` syntax in
     its tooltip and the option names listed (one per line, with each one's kind and values),
     validated live into the status label with `parseImportOptions`'s warnings. Stage 3 adds an
     *Options…* button with one widget per option.
  6. **Validate on save** (`yati_validate`): a check box. Tooltip: what the validation finds,
     where (the Issues view), and that it costs nothing when the map has no finding.
  7. A read-only line: how many types and members the extension manages (`yati_generated`),
     and that `yati_fixups` and `yati_generated` are the extension's records, not settings.
  8. A status area (a label spanning the row, red through `styleSheet` when it holds errors),
     then *Save* and *Cancel*.
  The glue calls `dialog.exec()`. *Save* validates (below); when something blocks, the status
  label names every problem and the dialog stays open; else the properties are written (only
  the ones that changed, as today), the dialog is accepted, and the report goes to the log and
  the status bar rather than another message box. *Cancel*, Escape and the window's close button
  write nothing.
- **Validation, before the dialog closes** (`validateConfiguration(values, io)` in
  `lib/configure.mjs`, `io` the glue's `exists`, `readText`, `run` checks, so a test can fake
  them). Blocking: the Godot project empty, missing, or without `project.godot`; a binary with a
  slash that does not exist; a scenes folder that does not exist; an import option with a bad
  boolean, a leftover that is not `name=value`, or `class_scenes`. Warnings (shown, but the
  dialog may close): a scenes folder outside the Godot project (the scan works, but the scenes
  get absolute paths); an unknown option name (passed on: a newer importer may know it); a
  binary that `--version` could not start or that is not Godot 4 (*Check map in Godot* will not
  work until it is fixed; the user may be configuring on a machine without Godot). The binary
  check runs on *Save* only when the binary changed or *Test* was never pressed, so a save is
  not slowed by a run that already happened.
- **Where the dialog is reached from.** The *Map* menu entry as now, and the other actions:
  when `requireSettings` finds no Godot project, it offers to open the dialog now
  (`confirm`), instead of telling the user to run another action. An action started without a
  project still gets the message that it needs one, with the menu path to open or make one.
- **The reopen, done by the extension.** `finishApply` keeps patching the file; then, instead
  of `REOPEN_TEXT`, the action's last statement is `tiled.open(project.fileName)` after a
  message that says the project is reloaded and open maps with unsaved changes will ask to be
  saved. Before it, nothing else may touch `project` (the `EditableProject` is replaced). Done
  or not is read back after the call: the patch writes the new `yati_fixups` record **into
  the file**, not through `setProperty`, so the in-memory project still holds the old record
  until the reload; `tiled.project.property("yati_fixups")` equal to the new record means the
  reload happened, else (the user cancelled a save prompt, or `open` threw: no editor) the old
  `REOPEN_TEXT` is shown and the record is set through the API as today, so the next action
  re-applies the fixes. In the harness (`--evaluate`) `open` throws and the fallback path is
  what runs, which the real-Tiled test asserts. The `yati_fixups` record stays for that
  fallback.
- **What does not change.** The settings' names, their storage, the README's table of them,
  `readSettings`, and the actions' behaviour past their dialogs. A project configured by the
  old prompts opens in the new dialog with its values. Tiled 1.12 stays the floor (`Dialog`'s
  tooltips are 1.12).

## 3. Stages and gates

### Stage 1: the dialog

- `lib/configure.mjs`: `FIELDS` (one record per setting: property name, label, kind, tooltip,
  placeholder, which read `readSettings` value fills it), `validateConfiguration`,
  `storedValues` (the dialog's values as the properties to write, using `storedPath` and
  `localToGodot`), and the explanation texts, so the glue has no prose of its own. The glue
  gets `buildConfigureDialog(api, settings)` returning the dialog, its widgets and its *Save*
  handler as a function, and `configure(api)` that builds it and calls `exec()`: a test can
  build, set the widgets and call the handler without `exec()`.
- `fake_tiled.mjs`: a `Dialog` fake that records the widgets each `add*` made (with `text`,
  `fileName`, `checked`, `currentIndex`, `toolTip`, `styleSheet`, `enabled`, `clicked.connect`,
  `click()`), `exec()` returning what a test scripted, `accept`/`reject`, and `promptOpenFile`.
  `api_surface.mjs`: the new members, each with its page (`classes/Dialog.html`,
  `classes/FileEdit.html`; the Qt widgets with `@mapeditor/tiled-api` 1.12.0's `index.d.ts`
  as their reference, since they have no page). `glue.test.mjs`: the dialog filled from a
  configured project; *Save* refused for each blocking case with the message in the status
  label and no property written; accepted for a good configuration with the expected
  properties (relative path, `res://` scenes folder, trimmed options); *Cancel* writes
  nothing; *Test* with a fake `Process` that prints a version; the other actions' offer to
  configure. `configure.test.mjs` for the pure module. `es_subset.test.mjs` already covers the
  new module's syntax.
- `real_tiled_harness.mjs`: a `dialog` step that builds the real dialog, sets the Godot
  project and the options through the widgets, presses *Save* with `click()` (never
  `exec()`), and logs the properties written; `real_tiled.test.mjs` asserts them. This is
  what confirms the widgets' members in Qt's engine; tooltips and layout are the checklist's.
- README: the *Settings* section describes the dialog, not the prompts; the manual checklist's
  step for *Configure…* lists what to see (each field's tooltip, a refused save, a saved one).
- Gate: `node --test tiled-extension/test` green (the API-surface test included), the real
  Tiled run green, the sample's project file unchanged.

### Stage 2: the reopen

- `apply.mjs`: `patchProjectText` also sets the project property `yati_fixups` in the file's
  `properties` (adding it when missing, with `"propertytype": ""` as Tiled writes it).
  `finishApply` and `report` as in section 2: the message names the reload; the reload is the
  last statement; the read-back decides between "reloaded" (logged) and the fallback.
- `fake_tiled.mjs`: `tiled.open` of the project file re-reads the fake project from the file
  the glue patched (types and properties), as `switchProject` does, so `glue.test.mjs` can
  assert that a sync followed by a second sync reports "Unchanged" with no reopen request, and
  that a fake whose `open` throws takes the fallback.
- `real_tiled.test.mjs`: the harness's `open` throws under `--evaluate`; the test asserts the
  fallback text once and the "Unchanged" second run as today.
- README: *What the API cannot set* says the extension reloads the project, what that does to
  open maps, and when the old instruction still applies. Checklist: after the sync, the
  project reloads, the open map is back, `GodotPhysicsLayers` shows the flags switch checked
  without a manual reopen; cancel a save prompt during the reload and see the fallback.
- Gate: the tests above; the checklist in the editor, by the user (nothing here can run the
  editor).

### Stage 3: the import options as fields

- An *Options…* button next to the options field opens a second `Dialog` with one row per
  option of `IMPORT_OPTIONS` but `class_scenes`: a combo box `(importer default) | true |
  false` for a boolean, a combo box of the known values for `parallax_mode`,
  `animation_mode`, `object_refs` and `unknown_class_fallback` (with a free-text fallback for
  a mapping-table class not listed), a text input for `custom_data_prefix` and
  `save_tileset_to`; each with Runtime.md's description as its tooltip. *OK* writes the
  `name=value` string back into the main dialog's field (so the stored form stays the string,
  readable by hand and by older versions). The values list comes from one table in
  `settings.mjs` (`IMPORT_OPTION_VALUES`) that the validator also uses to warn about an unknown
  value of a string option.
- Gate: the Node tests for the round trip (string to widgets to string), the glue test for the
  sub-dialog, README.

### Stage 4: the checklist in a real Tiled

- The user runs the README's checklist in Tiled 1.12: the dialog's look (field widths, the
  status label's wrapping, the tooltips), a refused save, the reload after a sync with a map
  open, the fallback when the reload is cancelled. Findings come back to section 4 here.

## 4. Status

- 2026-10-18: planned.
- 2026-10-18, **stage 1 done**: `lib/configure.mjs` (`FIELDS`, `TEXTS`, `initialValues`,
  `managedSummary`, `godotProjectStatus`, `godotVersionStatus`, `validateConfiguration`,
  `configuredLines`), `configureDialog(api, settings)` in the glue (builds the dialog without
  `exec()`; `configure()` calls `exec()`), `requireSettings` offering the dialog, the fake
  `Dialog` with its widgets (`answers.dialogs`: a function per `exec()`), `configure.test.mjs`
  and five glue tests, `api_surface.mjs` with the `Dialog`, `FileEdit` and Qt widget members
  (`index.d.ts#...` for the widgets without a page), the harness's `configure` and
  `configure:<options>` steps, the README. Decisions past section 2: the second *Save* is asked
  only for a warning the user's change brought (the warnings the dialog opened with, the binary
  not run, are saved at once), so a project that keeps an unknown option does not ask twice;
  `--version` runs once per binary text (the *Test* button's run is reused by *Save*); a save
  reports to the log only; an absent property stays absent for a default (`""`, `true`).
  **Confirmed in the real Tiled 1.12.0, headless** (`real_tiled.test.mjs`, the Tiled AppImage
  needing `libegl1`, `libxkbcommon-x11-0` and the `libxcb-*` set on the container): `Dialog` is
  a global and constructs without a main window; `FileEdit.fileName` reads a plain path;
  `QLineEdit.text` and `placeholderText`, `QCheckBox.checked`, `QLabel.text` and `toolTip` read
  and write; `QPushButton.click()` reaches the handler `clicked.connect` registered; a `Process`
  run from the handler; `setProperty` from the handler reaches the file; the sample's project
  file is unchanged by a save that changes nothing. Not confirmed, for the editor: the layout
  under `ManualRows` (the binary row with its two buttons, the status labels' wrapping), the
  red status, `exec()` returning on *Save*, `promptOpenFile` from *Browse…*.
