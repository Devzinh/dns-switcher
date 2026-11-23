; DNS Switcher installer (Inno Setup 6).
;
; Build:  cargo build --release
;         "C:\Program Files (x86)\Inno Setup 6\ISCC.exe" installer\dns-switcher.iss
; Output: installer\Output\DNS-Switcher-Setup-<version>.exe
;
; Installs the app to Program Files, registers the "DNS Switcher Service"
; (runs as LocalSystem, changes DNS on the app's behalf so the app itself never
; needs administrator rights), and adds Start menu entries and an uninstaller.

#define AppName "DNS Switcher"
; CI passes the version from Cargo.toml with /DAppVersion=x.y.z.
#ifndef AppVersion
  #define AppVersion "0.1.2"
#endif
#define AppExe "dns_switcher.exe"
#define ServiceName "DnsSwitcher"
#define RepoUrl "https://github.com/Devzinh/dns-switcher"

[Setup]
AppId={{6B0E5A52-3F7C-4C1D-9E2A-9A3D2C1B7E41}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=DNS Switcher contributors
AppPublisherURL={#RepoUrl}
AppSupportURL={#RepoUrl}/issues
AppUpdatesURL={#RepoUrl}/releases
AppCopyright=Copyright (c) 2026 DNS Switcher contributors. MIT License.
VersionInfoVersion={#AppVersion}.0
VersionInfoProductName={#AppName}
VersionInfoDescription={#AppName} Setup
VersionInfoCompany=DNS Switcher contributors
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
LicenseFile=..\LICENSE
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
OutputDir=Output
OutputBaseFilename=DNS-Switcher-Setup-{#AppVersion}
CloseApplications=no

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "ptbr"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[CustomMessages]
en.StartupTask=Start DNS Switcher in the notification area when I sign in
ptbr.StartupTask=Iniciar o DNS Switcher na área de notificação quando eu entrar no Windows
en.ServiceDescription=Changes DNS settings on behalf of the DNS Switcher app. Accepts requests only from users signed in to this computer.
ptbr.ServiceDescription=Altera as configurações de DNS a pedido do app DNS Switcher. Aceita pedidos apenas de usuários conectados neste computador.
en.LaunchApp=Open DNS Switcher
ptbr.LaunchApp=Abrir o DNS Switcher

[Tasks]
Name: "startup"; Description: "{cm:StartupTask}"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\CHANGELOG.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{group}\{cm:UninstallProgram,{#AppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
; The sign-in entry is per user, so the app writes it as the user who started
; setup, not as the administrator account that approved the UAC prompt.
Filename: "{app}\{#AppExe}"; Parameters: "--set-autostart"; Tasks: startup; Flags: runasoriginaluser runhidden waituntilterminated
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchApp}"; Flags: nowait postinstall skipifsilent runasoriginaluser
; Silent installs and upgrades (winget) bring the tray icon back right away.
Filename: "{app}\{#AppExe}"; Parameters: "--tray"; Tasks: startup; Flags: nowait skipifnotsilent runasoriginaluser

[UninstallRun]
Filename: "{app}\{#AppExe}"; Parameters: "--clear-autostart"; Flags: runhidden waituntilterminated; RunOnceId: "ClearAutostart"
Filename: "{sys}\taskkill.exe"; Parameters: "/f /im {#AppExe}"; Flags: runhidden; RunOnceId: "KillApp"
Filename: "{sys}\sc.exe"; Parameters: "stop {#ServiceName}"; Flags: runhidden; RunOnceId: "StopService"
Filename: "{sys}\sc.exe"; Parameters: "delete {#ServiceName}"; Flags: runhidden; RunOnceId: "DeleteService"

[UninstallDelete]
Type: dirifempty; Name: "{app}"

[Code]
function Sc(const Params: String): Integer;
var
  Code: Integer;
begin
  if not Exec(ExpandConstant('{sys}\sc.exe'), Params, '', SW_HIDE, ewWaitUntilTerminated, Code) then
    Code := -1;
  Result := Code;
end;

{ Stop the service and any running copy of the app so files can be replaced. }
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  Sc('stop {#ServiceName}');
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/f /im {#AppExe}', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Sleep(800);
  Result := '';
end;

{ Register (or update) and start the service. The binary path is quoted so a
  path with spaces can't be hijacked (unquoted service path). }
procedure CurStepChanged(CurStep: TSetupStep);
var
  BinPath: String;
begin
  if CurStep <> ssPostInstall then
    exit;
  BinPath := 'binPath= "\"' + ExpandConstant('{app}\{#AppExe}') + '\" --service"';
  if Sc('create {#ServiceName} ' + BinPath + ' start= auto DisplayName= "DNS Switcher Service"') <> 0 then
    Sc('config {#ServiceName} ' + BinPath + ' start= auto DisplayName= "DNS Switcher Service"');
  Sc('description {#ServiceName} "' + CustomMessage('ServiceDescription') + '"');
  Sc('failure {#ServiceName} reset= 86400 actions= restart/5000/restart/5000/none/0');
  Sc('start {#ServiceName}');
end;
