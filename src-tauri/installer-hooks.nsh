; ClipCat writes these files at runtime; remove them during uninstallation.
!macro NSIS_HOOK_POSTUNINSTALL
  ; Updates also run the old uninstaller: preserve the autostart entry.
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "ClipCat"
  ${EndIf}
  Delete "$INSTDIR\obs-ffmpeg-mux.exe"
  Delete "$INSTDIR\obs-nvenc-test.exe"
  ; Delete settings and logs only when "Delete application data" is checked.
  ${If} $DeleteAppDataCheckboxState = 1
    Delete "$INSTDIR\settings.json"
    RMDir /r "$LOCALAPPDATA\ClipCat"
  ${EndIf}
  RMDir "$INSTDIR"
!macroend
