; The Windows installer, built with Inno Setup 6 by `cargo xtask windows`,
; which passes the version, the folder of files to install, the icon and
; where to write the setup program as /D defines.
;
; It installs Nubo for the current user only (no administrator rights), in
; %LOCALAPPDATA%\Programs\Nubo, with a Start menu entry and an uninstaller.
; Running a newer installer updates an installed Nubo in place. Saves and
; settings live in %APPDATA%\Nubo and stay when Nubo is uninstalled.

#ifndef AppVersion
  #error Build the installer with `cargo xtask windows`, which defines AppVersion and the paths
#endif

[Setup]
; Identifies Nubo in Windows' list of installed apps; never change it, or
; installers stop recognizing an installed Nubo.
AppId={{E2ACFD90-07AA-4CF7-AE5A-9BF10F8F49AD}
AppName=Nubo
AppVersion={#AppVersion}
AppVerName=Nubo {#AppVersion}
AppPublisher=Michel Schoenmakers
AppPublisherURL=https://github.com/mschoenm/nubo
AppSupportURL=https://github.com/mschoenm/nubo/issues
AppUpdatesURL=https://github.com/mschoenm/nubo/releases
VersionInfoVersion={#AppVersion}
PrivilegesRequired=lowest
DefaultDirName={autopf}\Nubo
DisableDirPage=auto
DisableProgramGroupPage=yes
ArchitecturesAllowed={#Architectures}
ArchitecturesInstallIn64BitMode={#Architectures}
MinVersion=10.0
SetupIconFile={#IconFile}
UninstallDisplayIcon={app}\nubo.exe
UninstallDisplayName=Nubo
OutputDir={#OutputDir}
OutputBaseFilename={#OutputBaseFilename}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "associate"; Description: "Open Game Boy games (.gb and .gbc files) with Nubo"; GroupDescription: "Files:"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs

[Icons]
Name: "{autoprograms}\Nubo"; Filename: "{app}\nubo.exe"
Name: "{autodesktop}\Nubo"; Filename: "{app}\nubo.exe"; Tasks: desktopicon

; Offers Nubo in "Open with" for ROMs. Windows asks the player which app to
; use the first time, so this never takes the files away from another app.
[Registry]
Root: HKA; Subkey: "Software\Classes\Nubo.Rom"; ValueType: string; ValueName: ""; ValueData: "Game Boy game"; Flags: uninsdeletekey; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Nubo.Rom\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\nubo.exe,0"; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Nubo.Rom\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\nubo.exe"" ""%1"""; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.gb\OpenWithProgids"; ValueType: string; ValueName: "Nubo.Rom"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.gbc\OpenWithProgids"; ValueType: string; ValueName: "Nubo.Rom"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate

[Run]
Filename: "{app}\nubo.exe"; Description: "{cm:LaunchProgram,Nubo}"; Flags: nowait postinstall skipifsilent
