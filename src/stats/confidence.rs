//! Winrate suavizado (smoothing beta) para las filas del dataset de runas.
//!
//! Las celdas `campeón + posición + parche + runas` del dataset propio
//! arrancan casi vacías: con 2-5 partidas, el winrate crudo es ruido puro
//! (2/2 = 100 % no significa nada). El smoothing beta lo corrige tratando
//! cada fila como una media posterior de una Beta:
//!
//! ```text
//! P = (W + α) / (N + α + β)
//! ```
//!
//! Con `α = β = 10` ([`PRIOR_ALPHA`]/[`PRIOR_BETA`]) el prior equivale a 20
//! partidas "fantasma" al 50 %: una fila sin datos arranca exactamente en
//! 0.5 y se mueve hacia el winrate real a medida que `N` crece. La
//! influencia del prior es `20 / (N + 20)`: al llegar a [`MIN_GAMES`] (30)
//! partidas ya pesa solo un 40 % y con 100, un 17 %.
//!
//! [`MIN_GAMES`] = 30 es el umbral de confiabilidad de una fila (lo
//! consumirá el scoring de fases siguientes): es el mínimo estadístico
//! convencional para que el error estándar de una proporción sea tolerable
//! (`sqrt(0.25/30) ≈ 9 %`); por debajo, la media cruda no alcanza para
//! ordenar runas entre sí.

/// Peso del prior de victorias (pseudo-victorias al 50 %).
pub const PRIOR_ALPHA: f64 = 10.0;

/// Peso del prior de derrotas (pseudo-derrotas al 50 %). Igual a
/// [`PRIOR_ALPHA`] para un prior neutro centrado en 0.5.
pub const PRIOR_BETA: f64 = 10.0;

/// Mínimo de partidas para considerar confiable una fila del dataset (la
/// consumirá el scoring de fases siguientes; acá solo vive la constante y
/// [`is_reliable`]). Ver justificación en la doc del módulo.
pub const MIN_GAMES: u32 = 30;

/// Winrate suavizado: `(wins + α) / (games + α + β)`.
///
/// Puro y total: con `games = 0` devuelve el prior `α / (α + β)` (0.5 con
/// los valores por defecto), nunca divide por cero. `wins` mayor que
/// `games` (dato corrupto) produce un valor > 1 en vez de paniquear: el
/// scoring puede detectarlo, acá no se filtra.
#[must_use]
pub fn smoothed_winrate(wins: u32, games: u32, alpha: f64, beta: f64) -> f64 {
    (f64::from(wins) + alpha) / (f64::from(games) + alpha + beta)
}

/// ¿Hay suficientes partidas para confiar en la fila cruda (sin prior)?
#[must_use]
pub fn is_reliable(games: u32) -> bool {
    games >= MIN_GAMES
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ejemplo del spec: 12 wins / 20 games con α = β = 10 →
    /// (12 + 10) / (20 + 20) = 0.55.
    #[test]
    fn ejemplo_del_spec_doce_de_veinte() {
        let p = smoothed_winrate(12, 20, PRIOR_ALPHA, PRIOR_BETA);

        assert!((p - 0.55).abs() < 1e-9, "p = {p}");
    }

    /// Sin datos la fila arranca exactamente en el prior (0.5 con los
    /// valores por defecto): nunca divide por cero.
    #[test]
    fn cero_juegos_da_el_prior() {
        assert!((smoothed_winrate(0, 0, 10.0, 10.0) - 0.5).abs() < 1e-9);
        // Y con un prior asimétrico, el centro es α / (α + β).
        assert!((smoothed_winrate(0, 0, 1.0, 3.0) - 0.25).abs() < 1e-9);
    }

    /// 0 wins con 30 games sigue castigando la fila, pero menos que el
    /// winrate crudo (0.0): el prior la empuja hacia arriba.
    #[test]
    fn cero_wins_se_suaviza_hacia_arriba() {
        let p = smoothed_winrate(0, 30, PRIOR_ALPHA, PRIOR_BETA);

        assert!((p - 0.2).abs() < 1e-9, "p = {p}");
        assert!(p > 0.0);
    }

    /// Con muchas partidas el prior casi no pesa: 1000/1000 → 1010/1020.
    #[test]
    fn muestras_grandes_convergen_al_winrate_crudo() {
        let p = smoothed_winrate(1000, 1000, PRIOR_ALPHA, PRIOR_BETA);

        assert!((p - 1010.0 / 1020.0).abs() < 1e-9, "p = {p}");
        assert!(p > 0.98);
    }

    /// Con α = β = 0 el smoothing se apaga y queda el winrate crudo (y con
    /// `games = 0` la fórmula devolvería NaN: por eso el prior por defecto
    /// nunca es 0).
    #[test]
    fn prior_nulo_deja_el_winrate_crudo() {
        assert!((smoothed_winrate(12, 20, 0.0, 0.0) - 0.6).abs() < 1e-9);

        let nan = smoothed_winrate(0, 0, 0.0, 0.0);
        assert!(nan.is_nan(), "sin prior ni datos no hay valor");
    }

    /// El smoothing es monótono: más wins nunca baja P, con los mismos
    /// games.
    #[test]
    fn es_monotono_en_wins() {
        let mut prev = -1.0_f64;
        for wins in 0..=20_u32 {
            let p = smoothed_winrate(wins, 20, PRIOR_ALPHA, PRIOR_BETA);
            assert!(p >= prev, "wins={wins} baja P");
            prev = p;
        }
    }

    /// Umbral de confiabilidad: `MIN_GAMES` incluido, por debajo no.
    #[test]
    fn umbral_de_min_games() {
        assert!(!is_reliable(0));
        assert!(!is_reliable(MIN_GAMES - 1));
        assert!(is_reliable(MIN_GAMES));
        assert!(is_reliable(MIN_GAMES + 1));
    }

    /// Constantes del spec: prior simétrico 10/10 y umbral 30.
    #[test]
    fn constantes_del_spec() {
        assert_eq!(PRIOR_ALPHA, 10.0);
        assert_eq!(PRIOR_BETA, 10.0);
        assert_eq!(MIN_GAMES, 30);
    }
}
