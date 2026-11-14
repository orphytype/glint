; The Windows installer, built with Inno Setup 6 by scripts/dist-windows.ps1:
;   iscc /DAppVersion=0.1.0-alpha /DSourceExe=..\..\target\release\glint.exe /DOutDir=..\..\dist packaging\windows\glint.iss
; Installs for the current user in the fixed per-user Programs directory.
; The user's data (%APPDATA%\Glint, %LOCALAPPDATA%\Glint) stays when Glint is removed.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\..\target\release\glint.exe"
#endif
#ifndef OutDir
  #define OutDir "..\..\dist"
#endif

[Setup]
; Never change this id: it is how an update finds the installed copy.
AppId={{6E0B6E7A-3C55-4F4B-9A0E-6C1D5F0A7B21}
AppName=Glint
AppVersion={#AppVersion}
AppVerName=Glint {#AppVersion}
AppPublisher=Glint contributors
AppPublisherURL=https://github.com/orphytype/glint
AppSupportURL=https://github.com/orphytype/glint/issues
DefaultDirName={localappdata}\Programs\Glint
DefaultGroupName=Glint
DisableWelcomePage=yes
DisableDirPage=yes
DisableReadyPage=yes
DisableFinishedPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
SetupIconFile=..\..\crates\glint\assets\glint.ico
UninstallDisplayIcon={app}\glint.exe
UninstallDisplayName=Glint
OutputDir={#OutDir}
OutputBaseFilename=Glint-{#AppVersion}-windows-x64-setup
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "ru"; MessagesFile: "compiler:Languages\Russian.isl"

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "glint.exe"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\..\NOTICE.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
; The same AppUserModelID the app gives itself, so the taskbar groups its window under this shortcut
; and the media card names it.
Name: "{autoprograms}\Glint"; Filename: "{app}\glint.exe"; AppUserModelID: "Orphytype.Glint"
Name: "{autodesktop}\Glint"; Filename: "{app}\glint.exe"; AppUserModelID: "Orphytype.Glint"
