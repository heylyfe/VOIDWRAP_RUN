; VOIDWRAP RUN - Windows installer (NSIS 3, Modern UI 2, Unicode).
; Per-user install: no administrator rights are needed.
;
; Command line (called from tools/package.mjs):
;   makensis /DSRC=C:\path\to\dist\VOIDWRAP_RUN /DVERSION=0.1.0 /DOUTFILE=C:\path\to\setup.exe installer.nsi
; Paths in this script are relative to the script location (release\), so makensis is
; normally run with cwd = release\ (which is what tools/package.mjs does).

Unicode true

!include "MUI2.nsh"

!define APP_NAME "VOIDWRAP RUN"
!define APP_PUBLISHER "VOIDWRAP RUN"
!define APP_EXE "VOIDWRAP RUN.exe"
!define APP_UNINST "Uninstall VOIDWRAP RUN.exe"
!define APP_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\VOIDWRAP RUN"

!ifndef VERSION
  !define VERSION "dev"
!endif
!ifndef SRC
  !error "Pass the packaged game folder: /DSRC=C:\path\to\folder"
!endif
!ifndef OUTFILE
  !define OUTFILE "VOIDWRAP_RUN-${VERSION}-windows-setup.exe"
!endif

Name "${APP_NAME}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\${APP_NAME}"
InstallDirRegKey HKCU "${APP_KEY}" "InstallLocation"
RequestExecutionLevel user
SetShellVarContext current
SetCompressor /SOLID lzma

VIAddVersionKey "ProductName" "${APP_NAME}"
VIAddVersionKey "FileDescription" "${APP_NAME}"
VIAddVersionKey "CompanyName" "${APP_PUBLISHER}"
VIAddVersionKey "LegalCopyright" "${APP_NAME} contributors"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "OriginalFilename" "${APP_EXE}"

; App icon, both for the installer and the uninstaller (path relative to this script).
!define MUI_ICON "..\design\icon\icon.ico"
!define MUI_UNICON "..\design\icon\icon.ico"
!define MUI_BRANDINGTEXT "${APP_NAME}"
!define MUI_UNBRANDINGTEXT "${APP_NAME}"
!define MUI_ABORTWARNING
!define MUI_WELCOMEPAGE_TITLE "Welcome to VOIDWRAP RUN!"
!define MUI_WELCOMEPAGE_TEXT "This wizard will install VOIDWRAP RUN on your computer.$\r$\n$\r$\nVOIDWRAP RUN is a small gift-wrapped adventure for your Windows PC. No password or administrator rights are needed.$\r$\n$\r$\nClick Next to continue."
!define MUI_DIRECTORYPAGE_TEXT_TOP "Choose the folder where VOIDWRAP RUN should live. The default is a good choice for most people."
!define MUI_FINISHPAGE_TITLE "VOIDWRAP RUN is ready to play!"
!define MUI_FINISHPAGE_TEXT "VOIDWRAP RUN was installed on your computer.$\r$\n$\r$\nYou can find it in your Start menu, or use the checkbox below to play right away."
!define MUI_FINISHPAGE_RUN_FUNCTION LaunchGame
!define MUI_FINISHPAGE_RUN_TEXT "Start VOIDWRAP RUN now"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_UNLANGUAGE "English"

; If the game is open, ask the user to close it. _id keeps labels unique between the
; install and uninstall sections.
!macro EnsureGameClosed _id
  Push $0
  again_${_id}:
    FindWindow $0 "" "${APP_NAME}"
    IfErrors ok_${_id}
    MessageBox MB_YESNO|MB_ICONEXCLAMATION "VOIDWRAP RUN is open.$\r$\n$\r$\nPlease close it, then click Yes to continue.$\r$\nClick No to cancel." /SD IDNO IDYES again_${_id} IDNO abort_${_id}
  abort_${_id}:
    Pop $0
    Abort
  ok_${_id}:
    Pop $0
!macroend

Function LaunchGame
  ExecShell "" "$INSTDIR\${APP_EXE}"
FunctionEnd

Section "VOIDWRAP RUN" SEC_MAIN
  SectionIn RO
  !insertmacro EnsureGameClosed main
  SetOutPath "$INSTDIR"
  File /r "${SRC}\*.*"

  CreateDirectory "$SMPROGRAMS\${APP_NAME}"
  CreateShortCut "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0 SW_SHOWNORMAL "" "VOIDWRAP RUN"
  CreateShortCut "$SMPROGRAMS\${APP_NAME}\How to play.lnk" "$INSTDIR\HOW-TO-PLAY.md" "" "$INSTDIR\${APP_EXE}" 0 SW_SHOWNORMAL "" "How to play VOIDWRAP RUN"

  WriteUninstaller "$INSTDIR\${APP_UNINST}"

  WriteRegStr HKCU "${APP_KEY}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKCU "${APP_KEY}" "DisplayIcon" "$INSTDIR\${APP_EXE}"
  WriteRegStr HKCU "${APP_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${APP_KEY}" "Publisher" "${APP_PUBLISHER}"
  WriteRegStr HKCU "${APP_KEY}" "UninstallString" '"$INSTDIR\${APP_UNINST}"'
  WriteRegStr HKCU "${APP_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegDWORD HKCU "${APP_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${APP_KEY}" "NoRepair" 1
  WriteRegDWORD HKCU "${APP_KEY}" "EstimatedSize" 40960
SectionEnd

Section "Desktop shortcut" SEC_DESKTOP
  CreateShortCut "$DESKTOP\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0 SW_SHOWNORMAL "" "VOIDWRAP RUN"
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SEC_MAIN} "The game and everything it needs to run. Required."
  !insertmacro MUI_DESCRIPTION_TEXT ${SEC_DESKTOP} "Add a shortcut to your desktop so you can start VOIDWRAP RUN with a double-click."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  !insertmacro EnsureGameClosed uninst
  SetShellVarContext current
  Delete "$DESKTOP\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}\How to play.lnk"
  RMDir "$SMPROGRAMS\${APP_NAME}"
  DeleteRegKey HKCU "${APP_KEY}"
  ; The uninstaller cannot delete itself while it is running; /REBOOTOK asks Windows
  ; to remove whatever is left (the uninstaller itself) on the next restart.
  RMDir /r /REBOOTOK "$INSTDIR"
SectionEnd
