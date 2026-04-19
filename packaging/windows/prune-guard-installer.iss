#ifndef SourceBinary
  #error SourceBinary preprocessor define is required.
#endif

#ifndef SourceReadme
  #error SourceReadme preprocessor define is required.
#endif

#ifndef InstallerOutputDir
  #error InstallerOutputDir preprocessor define is required.
#endif

#ifndef InstallerBaseName
  #define InstallerBaseName "prune-guard-setup"
#endif

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{7A944CB8-658A-41FD-82AE-C9A51F5E6192}
AppName=prune-guard
AppVersion={#AppVersion}
AppPublisher=prune-guard
DefaultDirName={autopf}\prune-guard
DefaultGroupName=prune-guard
DisableProgramGroupPage=yes
OutputDir={#InstallerOutputDir}
OutputBaseFilename={#InstallerBaseName}
Compression=lzma
SolidCompression=yes
WizardStyle=classic
ChangesEnvironment=yes
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\prune-guard.exe

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "addtopath"; Description: "Add prune-guard install directory to the system PATH"; GroupDescription: "Additional tasks:"; Flags: unchecked

[Files]
Source: "{#SourceBinary}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceReadme}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\prune-guard CLI"; Filename: "{app}\prune-guard.exe"
Name: "{group}\Uninstall prune-guard"; Filename: "{uninstallexe}"

[Run]
Filename: "{app}\prune-guard.exe"; Parameters: "--help"; Description: "Run prune-guard --help"; Flags: postinstall nowait skipifsilent unchecked

[Code]
function NeedsAddPath(PathEntry: string): Boolean;
var
  ExistingPath: string;
  CanonicalPath: string;
begin
  CanonicalPath := ';' + Uppercase(PathEntry) + ';';
  if not RegQueryStringValue(
    HKLM,
    'SYSTEM\CurrentControlSet\Control\Session Manager\Environment',
    'Path',
    ExistingPath
  ) then
  begin
    Result := True;
    exit;
  end;

  ExistingPath := ';' + Uppercase(ExistingPath) + ';';
  Result := Pos(CanonicalPath, ExistingPath) = 0;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  ExistingPath: string;
  NewPath: string;
  InstallDir: string;
begin
  if CurStep <> ssPostInstall then
    exit;

  if not WizardIsTaskSelected('addtopath') then
    exit;

  InstallDir := ExpandConstant('{app}');
  if not NeedsAddPath(InstallDir) then
    exit;

  if not RegQueryStringValue(
    HKLM,
    'SYSTEM\CurrentControlSet\Control\Session Manager\Environment',
    'Path',
    ExistingPath
  ) then
  begin
    ExistingPath := '';
  end;

  NewPath := ExistingPath;
  if (Length(NewPath) > 0) and (NewPath[Length(NewPath)] <> ';') then
  begin
    NewPath := NewPath + ';';
  end;

  NewPath := NewPath + InstallDir;

  if not RegWriteExpandStringValue(
    HKLM,
    'SYSTEM\CurrentControlSet\Control\Session Manager\Environment',
    'Path',
    NewPath
  ) then
  begin
    RaiseException('Failed to update the system PATH for prune-guard.');
  end;
end;
