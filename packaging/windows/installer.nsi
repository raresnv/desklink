Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "WinVer.nsh"
!include "x64.nsh"
!define MUI_ICON "${APP_ICON}"
!define MUI_UNICON "${APP_ICON}"

!define PRODUCT_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\DeskLink"
!define PRODUCT_ID "dev.desklink.desktop.per-user"
Name "DeskLink ${VERSION} — Unsigned Preview"
OutFile "${OUTPUT}"
InstallDir "$LOCALAPPDATA\Programs\DeskLink"
RequestExecutionLevel user
SetCompressor /SOLID zlib
BrandingText "DeskLink — unsigned preview"
VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=1033 "ProductName" "DeskLink Installer — Unsigned Preview"
VIAddVersionKey /LANG=1033 "FileDescription" "DeskLink ${ARCH} Setup — Unsigned Preview"
VIAddVersionKey /LANG=1033 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "ProductVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "LegalCopyright" "DeskLink contributors — MIT license"
!define MUI_ABORTWARNING
!define MUI_WELCOMEPAGE_TITLE "DeskLink ${VERSION} — Unsigned Preview"
!define MUI_WELCOMEPAGE_TEXT "This preview is unsigned. Windows may show an unknown-publisher warning or block opening it. Publisher signing is not available yet.$\r$\n$\r$\nThis installs DeskLink ${ARCH} for your Windows account. Close DeskLink before installing or upgrading.$\r$\n$\r$\nYour computer ID and settings are retained. Every remote session still needs host approval."
!insertmacro MUI_PAGE_WELCOME
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE ValidateDirectory
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_TEXT "DeskLink is installed as an unsigned preview.$\r$\n$\r$\nOpen DeskLink from the Start menu. No remote session starts until the host approves it."
!define MUI_FINISHPAGE_RUN "$INSTDIR\desklink.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Open DeskLink"
!define MUI_FINISHPAGE_RUN_NOTCHECKED
!insertmacro MUI_PAGE_FINISH
!define MUI_UNCONFIRMPAGE_TEXT_TOP "Remove DeskLink for this Windows account? Close DeskLink first.$\r$\n$\r$\nYour computer ID, settings and unrelated files will be kept."
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_UNPAGE_FINISH
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  SetShellVarContext current
  ${IfNot} ${AtLeastWin10}
    MessageBox MB_OK|MB_ICONSTOP "DeskLink requires Windows 10 or later." /SD IDOK
    SetErrorLevel 2
    Quit
  ${EndIf}
  !if "${ARCH}" == "arm64"
    ${IfNot} ${IsNativeARM64}
      MessageBox MB_OK|MB_ICONSTOP "This installer requires Windows on ARM64. Use the x64 download for an x64 PC." /SD IDOK
      SetErrorLevel 2
      Quit
    ${EndIf}
    ${IfNot} ${AtLeastBuild} 22000
      MessageBox MB_OK|MB_ICONSTOP "The native ARM64 preview requires Windows 11." /SD IDOK
      SetErrorLevel 2
      Quit
    ${EndIf}
  !else
    ${IfNot} ${RunningX64}
      MessageBox MB_OK|MB_ICONSTOP "This installer requires 64-bit Windows." /SD IDOK
      SetErrorLevel 2
      Quit
    ${EndIf}
  !endif
  SetRegView 64
FunctionEnd

; Refuse unrelated nonempty folders. An owned folder permits reinstall/upgrade.
Function ValidateDirectory
  ReadINIStr $0 "$INSTDIR\desklink-install.ini" "DeskLink" "Product"
  ${If} $0 != "${PRODUCT_ID}"
    FindFirst $0 $1 "$INSTDIR\*"
    ${DoWhile} $1 != ""
      ${If} $1 != "."
      ${AndIf} $1 != ".."
        FindClose $0
        MessageBox MB_OK|MB_ICONSTOP "Choose an empty folder or a folder installed by this DeskLink installer. No existing files have been changed." /SD IDOK
        SetErrorLevel 2
        Abort
      ${EndIf}
      FindNext $0 $1
    ${Loop}
    FindClose $0
  ${EndIf}
FunctionEnd

!macro CheckClosed PREFIX
Function ${PREFIX}CheckClosed
  IfFileExists "$INSTDIR\desklink.exe" 0 done
  ClearErrors
  FileOpen $0 "$INSTDIR\desklink.exe" a
  ${If} ${Errors}
    MessageBox MB_OK|MB_ICONSTOP "Close DeskLink before continuing. The application may still be running, or this folder is not writable." /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}
  FileClose $0
  done:
FunctionEnd
!macroend
!insertmacro CheckClosed ""
!insertmacro CheckClosed "un."

Section "DeskLink ${ARCH} (unsigned preview)" Main
  SectionIn RO
  Call ValidateDirectory
  Call CheckClosed
  SetOutPath "$INSTDIR"
  WriteINIStr "$INSTDIR\desklink-install.ini" "DeskLink" "Product" "${PRODUCT_ID}"
  WriteINIStr "$INSTDIR\desklink-install.ini" "DeskLink" "Architecture" "${ARCH}"
  !include "${INSTALL_FILES}"
  SetOutPath "$INSTDIR"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateDirectory "$SMPROGRAMS\DeskLink"
  CreateShortcut "$SMPROGRAMS\DeskLink\DeskLink.lnk" "$INSTDIR\desklink.exe"
  CreateShortcut "$SMPROGRAMS\DeskLink\Uninstall DeskLink.lnk" "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "${PRODUCT_KEY}" "DisplayName" "DeskLink — Unsigned Preview (${ARCH})"
  WriteRegStr HKCU "${PRODUCT_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${PRODUCT_KEY}" "Publisher" "DeskLink project (unsigned preview)"
  WriteRegStr HKCU "${PRODUCT_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${PRODUCT_KEY}" "DisplayIcon" "$INSTDIR\desklink.exe"
  WriteRegStr HKCU "${PRODUCT_KEY}" "URLInfoAbout" "https://desklink.161.97.168.233.nip.io/"
  WriteRegStr HKCU "${PRODUCT_KEY}" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
  WriteRegStr HKCU "${PRODUCT_KEY}" "QuietUninstallString" '$\"$INSTDIR\Uninstall.exe$\" /S'
  WriteRegDWORD HKCU "${PRODUCT_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${PRODUCT_KEY}" "NoRepair" 1
  WriteRegDWORD HKCU "${PRODUCT_KEY}" "EstimatedSize" ${INSTALLED_KB}
SectionEnd

Function un.onInit
  SetShellVarContext current
  SetRegView 64
  ReadINIStr $0 "$INSTDIR\desklink-install.ini" "DeskLink" "Product"
  ${If} $0 != "${PRODUCT_ID}"
    MessageBox MB_OK|MB_ICONSTOP "This folder does not have a valid DeskLink installation marker. Nothing was removed." /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}
  Call un.CheckClosed
FunctionEnd

Section "Uninstall"
  SetOutPath "$TEMP"
  StrCpy $9 0
  ; Explicit build-time file list only: never recurse through an install folder.
  !include "${UNINSTALL_FILES}"
  ${If} $9 != 0
    MessageBox MB_OK|MB_ICONSTOP "Some DeskLink files could not be removed. Close DeskLink and retry. Your ID and settings have been kept." /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
  Delete "$SMPROGRAMS\DeskLink\DeskLink.lnk"
  Delete "$SMPROGRAMS\DeskLink\Uninstall DeskLink.lnk"
  RMDir "$SMPROGRAMS\DeskLink"
  DeleteRegKey HKCU "${PRODUCT_KEY}"
  Delete "$INSTDIR\desklink-install.ini"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
SectionEnd
