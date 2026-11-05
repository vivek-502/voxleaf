Unicode true
!include MUI2.nsh
!include x64.nsh

!define PRODUCTNAME "VoxLeaf"
!define VERSION "1.0.0"
!define MAINBINARYNAME "voxleaf"
!define OUTFILE "voxleaf-1.0.0-x86_64-pc-windows-gnu-setup.exe"
!define INSTALLMODE "currentUser"

RequestExecutionLevel user

Name "${PRODUCTNAME} ${VERSION}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\\${PRODUCTNAME}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCTNAME}"
VIAddVersionKey "FileDescription" "${PRODUCTNAME}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_LANGUAGE "English"

Section Install
  SetOutPath $INSTDIR
  File "/home/arch/My-Works/Desktop-apps/nov-2026/voxleaf/target/x86_64-pc-windows-gnu/release/voicebook-desktop.exe"
  File "/home/arch/My-Works/Desktop-apps/nov-2026/voxleaf/target/x86_64-pc-windows-gnu/release/WebView2Loader.dll"
  WriteUninstaller "$INSTDIR\\uninstall.exe"
  WriteRegStr HKCU "Software\\${PRODUCTNAME}" "" $INSTDIR
  WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\${PRODUCTNAME}" "DisplayName" "${PRODUCTNAME}"
  WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\${PRODUCTNAME}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\${PRODUCTNAME}" "Publisher" "vivek-502"
  WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\${PRODUCTNAME}" "InstallLocation" "$\"$INSTDIR$\""
  WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\${PRODUCTNAME}" "UninstallString" "$\"$INSTDIR\\uninstall.exe$\""
  WriteRegDWORD HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\${PRODUCTNAME}" "NoModify" "1"
  WriteRegDWORD HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\${PRODUCTNAME}" "NoRepair" "1"
SectionEnd

Section Uninstall
  Delete "$INSTDIR\\${MAINBINARYNAME}.exe"
  Delete "$INSTDIR\\WebView2Loader.dll"
  Delete "$INSTDIR\\uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\${PRODUCTNAME}"
  DeleteRegKey HKCU "Software\\${PRODUCTNAME}"
SectionEnd
