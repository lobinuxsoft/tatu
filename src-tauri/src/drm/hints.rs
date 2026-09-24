use super::types::Preservability;

/// Spanish human-readable hint describing the preservability level and the
/// concrete action the user can take.
pub(super) fn preservability_hint(pres: &Preservability) -> String {
    match pres {
        Preservability::Trivial => "Preservación trivial: el juego no tiene DRM. Copiá la carpeta \
            de steamapps/common/<juego> a otro disco. No requiere herramientas."
            .into(),
        Preservability::Easy => "Compatible con Goldberg Emulator: el juego solo usa el wrapper de \
            Steam DRM. Con Goldberg (reemplazo de steam_api.dll) más Steamless (si tiene SteamStub) \
            corre offline sin el cliente de Steam."
            .into(),
        Preservability::Alternative => "Disponible DRM-free en GOG: alternativa oficial y legal \
            sin DRM. Considerá comprarlo/reclamarlo en GOG para tener una copia portable y \
            preservable sin depender de Steam."
            .into(),
        Preservability::Removed { removed_vendors } => format!(
            "DRM removido oficialmente: el publisher removió {} de la versión actual. La copia de \
             Steam ya es directamente preservable sin DRM activo.",
            removed_vendors.join(", ")
        ),
        Preservability::Hard => "Preservación compleja: el juego tiene DRM embebido activo sin \
            alternativa limpia documentada. Requeriría un crack específico del vendor — fuera del \
            alcance de esta herramienta."
            .into(),
        Preservability::Unknown => "Preservabilidad desconocida: sin datos suficientes para \
            clasificar. Puede variar desde trivial hasta compleja — refrescá los datos de DRM o \
            consultá manualmente en PCGamingWiki."
            .into(),
    }
}
