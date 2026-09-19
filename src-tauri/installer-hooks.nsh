; A ClipCat futas kozben maga irja ezeket; az eltavolitas ne hagyja ott oket.
!macro NSIS_HOOK_POSTUNINSTALL
  ; Frissiteskor a regi eltavolito is lefut: az automatikus inditas maradjon
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "ClipCat"
  ${EndIf}
  Delete "$INSTDIR\obs-ffmpeg-mux.exe"
  Delete "$INSTDIR\obs-nvenc-test.exe"
  ; A beallitasok es a naplo csak akkor torlodik, ha az "alkalmazasadatok torlese" be van jelolve
  ${If} $DeleteAppDataCheckboxState = 1
    Delete "$INSTDIR\settings.json"
    RMDir /r "$LOCALAPPDATA\ClipCat"
  ${EndIf}
  RMDir "$INSTDIR"
!macroend
