; Hotview per-user installer (NSIS 3).
;
; Build (on Windows, from the repository root):
;   makensis /DVERSION=1.0.36 /DAPPDIR=C:\path\to\dist\windows\app ^
;            /DOUTFILE=C:\path\to\Hotview-1.0.36-x64-setup.exe ^
;            packaging\windows\installer.nsi
;
; The app directory must already contain hotview.exe plus every DLL it needs
; (FFmpeg and the MinGW runtime closure) as produced by the desktop workflow.

Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"

!ifndef VERSION
    !define VERSION "1.0.0"
!endif
!ifndef APPDIR
    !define APPDIR "${__FILEDIR__}/../../dist/windows/app"
!endif
!ifndef OUTFILE
    !define OUTFILE "${__FILEDIR__}/../../dist/Hotview-${VERSION}-x64-setup.exe"
!endif
; `File /r` is happiest with native separators on Windows. Override this
; define when smoke-testing the script with makensis on Linux.
!ifndef APP_GLOB
    !define APP_GLOB "${APPDIR}\*.*"
!endif

!define APP_NAME "Hotview"
!define APP_EXE "hotview.exe"
!define APP_PUBLISHER "Hotsteel"
!define APP_URL "https://github.com/Hotsteel2901/Hotview"
!define APP_ID "Hotview"
!ifndef ICON_FILE
    !define ICON_FILE "${__FILEDIR__}/../icons/hotview.ico"
!endif
!ifndef LICENSE_FILE
    !define LICENSE_FILE "${__FILEDIR__}/../../LICENSE"
!endif

Name "${APP_NAME} ${VERSION}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\Hotview"
InstallDirRegKey HKCU "Software\Hotview" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
SetCompressorDictSize 64
ShowInstDetails nevershow
ShowUninstDetails nevershow
BrandingText "${APP_NAME} ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=0 "ProductName" "${APP_NAME}"
VIAddVersionKey /LANG=0 "ProductVersion" "${VERSION}"
VIAddVersionKey /LANG=0 "FileDescription" "${APP_NAME} Setup"
VIAddVersionKey /LANG=0 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=0 "LegalCopyright" "Copyright (C) 2026 Hotsteel"
VIAddVersionKey /LANG=0 "CompanyName" "${APP_PUBLISHER}"

!define MUI_ICON "${ICON_FILE}"
!define MUI_UNICON "${ICON_FILE}"
!define MUI_ABORTWARNING
!define MUI_LANGDLL_REGISTRY_ROOT "HKCU"
!define MUI_LANGDLL_REGISTRY_KEY "Software\Hotview"
!define MUI_LANGDLL_REGISTRY_VALUENAME "InstallerLanguage"
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Launch Hotview / 启动 Hotview"
!define MUI_WELCOMEPAGE_TITLE "Welcome to Hotview ${VERSION}"
!define MUI_WELCOMEPAGE_TEXT "Hotview is a fast image and video viewer.$\r$\n$\r$\nThis installer is per-user and does not need administrator rights.$\r$\n百度云 / 中文用户：安装无需管理员权限，装好即可直接使用。"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "${LICENSE_FILE}"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "SimpChinese"

!macro RegisterExtension EXT PROGID DESCRIPTION
    WriteRegStr HKCU "Software\Classes\${PROGID}" "" "${DESCRIPTION}"
    WriteRegStr HKCU "Software\Classes\${PROGID}\DefaultIcon" "" "$INSTDIR\hotview.ico,0"
    WriteRegStr HKCU "Software\Classes\${PROGID}\shell\open\command" "" '"$INSTDIR\${APP_EXE}" "%1"'
    WriteRegStr HKCU "Software\Classes\${EXT}\OpenWithProgids" "${PROGID}" ""
    WriteRegStr HKCU "Software\Hotview\Capabilities\FileAssociations" "${EXT}" "${PROGID}"
!macroend

!macro UnregisterExtension EXT PROGID
    DeleteRegKey HKCU "Software\Classes\${PROGID}"
    DeleteRegValue HKCU "Software\Classes\${EXT}\OpenWithProgids" "${PROGID}"
    DeleteRegValue HKCU "Software\Hotview\Capabilities\FileAssociations" "${EXT}"
!macroend

Function .onInit
    ; Per-user install: shortcuts and registry go to the current user.
    SetShellVarContext current
    ; Write the Add/Remove Programs entry into the 64-bit registry view.
    SetRegView 64
    ${IfNot} ${Silent}
        !insertmacro MUI_LANGDLL_DISPLAY
    ${EndIf}
FunctionEnd

Section "Hotview (required)" SEC_MAIN
    SectionIn RO

    ; Replace a running copy so the upgrade can overwrite the executable.
    nsExec::Exec '"$SYSDIR\taskkill.exe" /IM ${APP_EXE} /F'
    Pop $0

    SetOutPath "$INSTDIR"
    File /r "${APP_GLOB}"

    WriteUninstaller "$INSTDIR\Uninstall.exe"
    WriteRegStr HKCU "Software\Hotview" "InstallDir" "$INSTDIR"

    CreateDirectory "$SMPROGRAMS\Hotview"
    CreateShortcut "$SMPROGRAMS\Hotview\Hotview.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\hotview.ico" 0
    CreateShortcut "$SMPROGRAMS\Hotview\Uninstall Hotview.lnk" "$INSTDIR\Uninstall.exe" "" "$INSTDIR\hotview.ico" 0

    ; Add/Remove Programs entry (per-user).
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Hotview" \
        "DisplayName" "${APP_NAME}"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Hotview" \
        "DisplayVersion" "${VERSION}"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Hotview" \
        "DisplayIcon" "$INSTDIR\hotview.ico,0"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Hotview" \
        "Publisher" "${APP_PUBLISHER}"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Hotview" \
        "URLInfoAbout" "${APP_URL}"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Hotview" \
        "InstallLocation" "$INSTDIR"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Hotview" \
        "UninstallString" '"$INSTDIR\Uninstall.exe"'
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Hotview" \
        "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
    WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Hotview" \
        "NoModify" 1
    WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Hotview" \
        "NoRepair" 1
    WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Hotview" \
        "EstimatedSize" 250000

    ; "Open with" registration and Default Apps capabilities. This never
    ; hijacks an existing default; the user can pick Hotview in Windows
    ; Settings -> Default apps.
    WriteRegStr HKCU "Software\Classes\Applications\${APP_EXE}\shell\open\command" \
        "" '"$INSTDIR\${APP_EXE}" "%1"'
    WriteRegStr HKCU "Software\Classes\Applications\${APP_EXE}" "FriendlyAppName" "${APP_NAME}"
    WriteRegStr HKCU "Software\Hotview\Capabilities" "ApplicationName" "${APP_NAME}"
    WriteRegStr HKCU "Software\Hotview\Capabilities" "ApplicationDescription" \
        "Fast image and video viewer with GPU rendering"
    WriteRegStr HKCU "Software\RegisteredApplications" "Hotview" \
        "Software\Hotview\Capabilities"

    !insertmacro RegisterExtension ".jpg" "Hotview.jpg" "JPEG image"
    !insertmacro RegisterExtension ".jpeg" "Hotview.jpeg" "JPEG image"
    !insertmacro RegisterExtension ".png" "Hotview.png" "PNG image"
    !insertmacro RegisterExtension ".gif" "Hotview.gif" "GIF image"
    !insertmacro RegisterExtension ".webp" "Hotview.webp" "WebP image"
    !insertmacro RegisterExtension ".bmp" "Hotview.bmp" "BMP image"
    !insertmacro RegisterExtension ".tif" "Hotview.tif" "TIFF image"
    !insertmacro RegisterExtension ".tiff" "Hotview.tiff" "TIFF image"
    !insertmacro RegisterExtension ".ico" "Hotview.ico" "Icon"
    !insertmacro RegisterExtension ".avif" "Hotview.avif" "AVIF image"
    !insertmacro RegisterExtension ".heic" "Hotview.heic" "HEIC image"
    !insertmacro RegisterExtension ".mp4" "Hotview.mp4" "MP4 video"
    !insertmacro RegisterExtension ".mkv" "Hotview.mkv" "Matroska video"
    !insertmacro RegisterExtension ".webm" "Hotview.webm" "WebM video"
    !insertmacro RegisterExtension ".mov" "Hotview.mov" "QuickTime video"
    !insertmacro RegisterExtension ".avi" "Hotview.avi" "AVI video"
    !insertmacro RegisterExtension ".m4v" "Hotview.m4v" "MP4 video"
SectionEnd

Section "Desktop shortcut / 桌面快捷方式" SEC_DESKTOP
    CreateShortcut "$DESKTOP\Hotview.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\hotview.ico" 0
SectionEnd

LangString DESC_SEC_MAIN ${LANG_ENGLISH} "Hotview program files, Start Menu shortcut and file registration."
LangString DESC_SEC_MAIN ${LANG_SIMPCHINESE} "Hotview 程序文件、开始菜单快捷方式与文件关联。"
LangString DESC_SEC_DESKTOP ${LANG_ENGLISH} "Create a shortcut on the desktop."
LangString DESC_SEC_DESKTOP ${LANG_SIMPCHINESE} "在桌面创建快捷方式。"

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
!insertmacro MUI_DESCRIPTION_TEXT ${SEC_MAIN} $(DESC_SEC_MAIN)
!insertmacro MUI_DESCRIPTION_TEXT ${SEC_DESKTOP} $(DESC_SEC_DESKTOP)
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
    SetShellVarContext current
    SetRegView 64
    nsExec::Exec '"$SYSDIR\taskkill.exe" /IM ${APP_EXE} /F'
    Pop $0

    Delete "$DESKTOP\Hotview.lnk"
    RMDir /r "$SMPROGRAMS\Hotview"

    ; File registration.
    !insertmacro UnregisterExtension ".jpg" "Hotview.jpg"
    !insertmacro UnregisterExtension ".jpeg" "Hotview.jpeg"
    !insertmacro UnregisterExtension ".png" "Hotview.png"
    !insertmacro UnregisterExtension ".gif" "Hotview.gif"
    !insertmacro UnregisterExtension ".webp" "Hotview.webp"
    !insertmacro UnregisterExtension ".bmp" "Hotview.bmp"
    !insertmacro UnregisterExtension ".tif" "Hotview.tif"
    !insertmacro UnregisterExtension ".tiff" "Hotview.tiff"
    !insertmacro UnregisterExtension ".ico" "Hotview.ico"
    !insertmacro UnregisterExtension ".avif" "Hotview.avif"
    !insertmacro UnregisterExtension ".heic" "Hotview.heic"
    !insertmacro UnregisterExtension ".mp4" "Hotview.mp4"
    !insertmacro UnregisterExtension ".mkv" "Hotview.mkv"
    !insertmacro UnregisterExtension ".webm" "Hotview.webm"
    !insertmacro UnregisterExtension ".mov" "Hotview.mov"
    !insertmacro UnregisterExtension ".avi" "Hotview.avi"
    !insertmacro UnregisterExtension ".m4v" "Hotview.m4v"

    DeleteRegKey HKCU "Software\Classes\Applications\${APP_EXE}"
    DeleteRegValue HKCU "Software\RegisteredApplications" "Hotview"
    DeleteRegKey HKCU "Software\Hotview\Capabilities"
    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Hotview"
    DeleteRegKey HKCU "Software\Hotview"

    ; Remove the installation last so Uninstall.exe can finish executing.
    RMDir /r "$INSTDIR"
SectionEnd
