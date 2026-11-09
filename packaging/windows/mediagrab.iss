; Inno Setup script -> MediaGrab-<version>-windows-x64-setup.exe
; Run from the repo root:  iscc packaging\windows\mediagrab.iss   (env APP_VERSION optional)
#define AppName "MediaGrab"
#define AppVersion GetEnv("APP_VERSION")
#if AppVersion == ""
  #define AppVersion "0.1.0"
#endif

[Setup]
AppId={{5ADA796B-89D0-4F0D-8F8C-20E78C0E49D9}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=MediaGrab
AppPublisherURL=https://github.com/RushabhShahPrograms/MediaGrab
AppSupportURL=https://github.com/RushabhShahPrograms/MediaGrab/issues
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
; Per-user install: no admin prompt, and the bundled yt-dlp can update itself (folder is writable).
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir=..\..\dist
OutputBaseFilename=MediaGrab-{#AppVersion}-windows-x64-setup
SetupIconFile=..\..\crates\mediagrab-app\assets\icon.ico
UninstallDisplayIcon={app}\mediagrab.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"

[Files]
Source: "..\..\target\release\mediagrab.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\dist\tools\*"; DestDir: "{app}\tools"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\mediagrab.exe"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\mediagrab.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\mediagrab.exe"; Description: "Launch {#AppName}"; Flags: nowait postinstall skipifsilent
