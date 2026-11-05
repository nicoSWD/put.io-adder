; Inno Setup script. Compile with:
;   iscc /DAppVersion=1.2.3 /DArch=x64 /DSourceDir=dist\windows-x64 packaging\windows\putio-adder.iss
; Installs per user (no administrator rights) and registers put.io adder as an *available* handler
; for magnet links and .torrent files. Windows 10/11 only lets the user choose the default, so the
; installer offers to open "Default apps" at the end.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Arch
  #define Arch "x64"
#endif
#ifndef SourceDir
  #define SourceDir "dist"
#endif

[Setup]
AppId={{6F1D9E0A-4B7C-4E55-9B1A-5F3C2D8E7A41}
AppName=put.io adder
AppVersion={#AppVersion}
AppPublisher=Nicolas Oelgart
AppPublisherURL=https://github.com/nicoSWD/putio-adder-rs
DefaultDirName={autopf}\put.io adder
DefaultGroupName=put.io adder
PrivilegesRequired=lowest
Compression=lzma2
SolidCompression=yes
OutputDir=..\..\dist
OutputBaseFilename=putio-adder-v{#AppVersion}-windows-{#Arch}-setup
SetupIconFile={#SourceDir}\putio-adder.ico
UninstallDisplayIcon={app}\putio-adder.exe
LicenseFile=..\..\LICENSE
WizardStyle=modern
#if Arch == "arm64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif

[Files]
Source: "{#SourceDir}\putio-adder.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\putio-adder.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\assets\fonts\OFL-Montserrat.txt"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "{#SourceDir}\THIRD_PARTY.txt"; DestDir: "{app}\licenses"; Flags: ignoreversion skipifsourcedoesntexist

[Icons]
Name: "{group}\put.io adder"; Filename: "{app}\putio-adder.exe"; IconFilename: "{app}\putio-adder.ico"

[Registry]
; ProgIDs belong to us, so they are removed on uninstall.
Root: HKCU; Subkey: "Software\Classes\putio-adder.magnet"; ValueType: string; ValueData: "URL:magnet Protocol"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\putio-adder.magnet"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\putio-adder.magnet\DefaultIcon"; ValueType: string; ValueData: "{app}\putio-adder.ico"
Root: HKCU; Subkey: "Software\Classes\putio-adder.magnet\shell\open\command"; ValueType: string; ValueData: """{app}\putio-adder.exe"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\putio-adder.torrent"; ValueType: string; ValueData: "BitTorrent Document"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\putio-adder.torrent\DefaultIcon"; ValueType: string; ValueData: "{app}\putio-adder.ico"
Root: HKCU; Subkey: "Software\Classes\putio-adder.torrent\shell\open\command"; ValueType: string; ValueData: """{app}\putio-adder.exe"" ""%1"""
; "Open with" list for .torrent files, without taking over the default.
Root: HKCU; Subkey: "Software\Classes\.torrent\OpenWithProgids"; ValueType: string; ValueName: "putio-adder.torrent"; ValueData: ""; Flags: uninsdeletevalue
; Capabilities make the app show up under Settings > Default apps.
Root: HKCU; Subkey: "Software\putio-adder\Capabilities"; ValueType: string; ValueName: "ApplicationName"; ValueData: "put.io adder"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\putio-adder\Capabilities"; ValueType: string; ValueName: "ApplicationDescription"; ValueData: "Send magnet links and .torrent files to put.io"
Root: HKCU; Subkey: "Software\putio-adder\Capabilities\URLAssociations"; ValueType: string; ValueName: "magnet"; ValueData: "putio-adder.magnet"
Root: HKCU; Subkey: "Software\putio-adder\Capabilities\FileAssociations"; ValueType: string; ValueName: ".torrent"; ValueData: "putio-adder.torrent"
Root: HKCU; Subkey: "Software\RegisteredApplications"; ValueType: string; ValueName: "putio-adder"; ValueData: "Software\putio-adder\Capabilities"; Flags: uninsdeletevalue

[Run]
Filename: "ms-settings:defaultapps"; Description: "Choose put.io adder as the default for magnet links and .torrent files"; Flags: shellexec postinstall skipifsilent unchecked
Filename: "{app}\putio-adder.exe"; Description: "Start put.io adder"; Flags: nowait postinstall skipifsilent
