// Le champ vivant, derrière l'atelier.
//
// Trois populations de lumière sur la nuit.
//
// La **grille** est une trame de points fixes, à peine visible : le plan sur lequel tout se
// pose. La **poussière** est une voûte de grains fixes, plus dense le long d'une houle qui
// traverse l'écran : elle donne la profondeur et ne bouge jamais. Un écran au repos est un écran
// immobile.
//
// Les **courants** sont les missions. Chaque courant est un ruban de particules tissé de
// plusieurs fils, qui avancent à la vitesse réelle de la mission : un agent qui franchit ses
// étapes fait filer son ruban, un agent arrêté le fige sur place. La clarté dit ce qui reste de
// budget ; la teinte passe à l'alerte quand un humain est attendu ; l'accent éclaire la mission
// choisie. La couleur du champ est celle de l'accent choisi par la personne.
//
// Rien ici n'est décoratif. Si une particule bouge, c'est qu'une étape a été franchie.

struct Cadre {
    resolution: vec2<f32>,
    temps: f32,
    attenuation: f32,
    poussiere: u32,
    par_courant: u32,
    grille: u32,
    rubans: u32,
    accent: vec3<f32>,
    _r0: f32,
    alerte: vec3<f32>,
    _r1: f32,
};

struct Courant {
    // Hauteur de base du ruban, en fraction de l'écran.
    base: f32,
    // Amplitude de son ondulation.
    amplitude: f32,
    // Vitesse d'avance des particules, en largeurs d'écran par seconde.
    vitesse: f32,
    // Clarté, de 0 à 1.
    clarte: f32,
    // Décalage de phase, pour que deux courants ne se superposent pas.
    phase: f32,
    // Teinte : 0 = accent, 1 = l'alerte réservée à ce qui attend un humain.
    teinte: f32,
    // Accent : 1 pour la mission choisie, 0 sinon.
    accent: f32,
    // Largeur du ruban, en fraction de la hauteur d'écran.
    largeur: f32,
};

@group(0) @binding(0) var<uniform> cadre: Cadre;
@group(0) @binding(1) var<storage, read> courants: array<Courant>;

struct Sortie {
    @builtin(position) position: vec4<f32>,
    @location(0) centre: vec2<f32>,
    @location(1) couleur: vec4<f32>,
};

// Un bruit bon marché et reproductible. Reproductible importe : deux rendus de la même scène au
// même instant doivent donner la même image, sans quoi rien ne se compare.
fn aleatoire(graine: f32) -> f32 {
    return fract(sin(graine * 12.9898) * 43758.5453);
}

// La courbe que suit un ruban. Une pente qui monte vers la droite, puis trois harmoniques :
// une donne un arc, deux donnent une vague, trois donnent quelque chose qui ne se répète pas
// à l'œil.
fn courbe(x: f32, c: Courant) -> f32 {
    return c.base
        - 0.17 * (x - 0.5)
        + c.amplitude * sin(x * 3.1 + c.phase)
        + c.amplitude * 0.45 * sin(x * 7.3 - c.phase * 1.7)
        + c.amplitude * 0.20 * sin(x * 13.1 + c.phase * 0.6);
}

// La bande où la poussière se concentre : une houle lente qui monte vers la droite, comme les
// rubans qu'elle accompagne.
fn bande(x: f32) -> f32 {
    return 0.60 - 0.14 * x + 0.09 * sin(x * 4.2 + 1.1) + 0.04 * sin(x * 9.7);
}

const BLANC: vec3<f32> = vec3<f32>(1.0, 1.0, 1.0);
// Pas de la grille, en pixels.
const PAS_GRILLE: f32 = 56.0;

@vertex
fn vs(
    @builtin(vertex_index) sommet: u32,
    @builtin(instance_index) instance: u32,
) -> Sortie {
    let coins = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
    );
    let coin = coins[sommet];
    let graine = f32(instance) + 1.0;
    let fin_poussiere = cadre.poussiere;
    let fin_rubans = fin_poussiere + cadre.rubans * cadre.par_courant;

    var centre_px = vec2<f32>(0.0, 0.0);
    var rayon_px = 1.0;
    var teinte = cadre.accent;
    var intensite = 0.0;

    if (instance < fin_poussiere) {
        // --- La poussière : fixe, tirée une fois pour toutes de sa graine. ---
        let x = aleatoire(graine * 1.31);
        let y = aleatoire(graine * 2.71);
        let ecart = y - bande(x);
        let proche = exp(-(ecart * ecart) / 0.024);
        let tirage = aleatoire(graine * 3.37);
        // Les quelques grains les plus brillants tirent vers le blanc : une voûte n'est pas
        // d'une seule couleur.
        teinte = mix(cadre.accent, BLANC, step(0.93, tirage) * 0.6);
        rayon_px = 0.8 + tirage * 1.5;
        var force = (0.16 + 0.70 * aleatoire(graine * 4.13)) * (0.10 + 0.90 * proche);
        if (tirage > 0.95) {
            // Un halo large et faible, pour la profondeur.
            rayon_px = 3.0 + tirage * 8.0;
            force = force * 0.22;
        }
        intensite = force * cadre.attenuation;
        centre_px = vec2<f32>(x * cadre.resolution.x, y * cadre.resolution.y);
    } else if (instance < fin_rubans) {
        // --- Les courants : un ruban par mission, tissé de treize fils. ---
        let local = instance - fin_poussiere;
        let c = courants[local / cadre.par_courant];
        let fil = f32(local % 13u) - 6.0;
        let tirage = aleatoire(graine * 3.3);
        let phase_particule = aleatoire(graine);
        let ecart = (aleatoire(graine * 1.7) - 0.5) * 2.0;
        let largeur = c.largeur * (1.0 + 0.5 * c.accent);
        // Le fil donne sa place à la particule ; le tirage ne l'en écarte qu'un peu, pour que
        // chaque fil reste une ligne de lumière et que le ruban soit tissé, pas brumeux.
        let lateral = ecart * ecart * ecart * largeur * 0.4 + fil * largeur * 0.15;

        // L'avance. `fract` rebouclerait brutalement ; on atténue plutôt aux deux bords.
        let t = fract(phase_particule + cadre.temps * c.vitesse);
        let x = t;
        // Chaque fil ondule un peu à sa manière : c'est ce qui tisse.
        let y = courbe(x, c) + lateral
            + 0.014 * sin(x * 21.0 + fil * 1.3 + c.phase)
            + 0.006 * sin(x * 47.0 - fil * 0.7);

        // Deux populations. Un cœur fin et vif porte le dessin ; un halo large et faible lui
        // donne sa profondeur. Une taille unique produit soit une ligne dure, soit une brume —
        // jamais une lumière.
        let est_halo = tirage > 0.88;
        rayon_px = 0.9 + tirage * 1.8;
        var force = 1.5 + 0.7 * c.accent;
        if (est_halo) {
            rayon_px = 4.0 + tirage * 10.0;
            force = 0.16 + 0.08 * c.accent;
        }
        // Un ruban naît et meurt au lieu de se couper net.
        let bord = smoothstep(0.0, 0.14, t) * (1.0 - smoothstep(0.84, 1.0, t));
        // Un grain le long du fil : il suit la particule, donc il se fige avec elle. Un
        // scintillement horloger animerait une mission arrêtée, ce qui serait un mensonge.
        let grain = 0.70 + 0.30 * sin(t * 18.85 + graine);
        teinte = mix(mix(cadre.accent, BLANC, 0.25 * c.accent), cadre.alerte, c.teinte);
        intensite = c.clarte * bord * grain * force * cadre.attenuation;
        centre_px = vec2<f32>(x * cadre.resolution.x, y * cadre.resolution.y);
    } else {
        // --- La grille : une trame de points, le plan sur lequel tout se pose. ---
        let g = instance - fin_rubans;
        let colonnes = u32(ceil(cadre.resolution.x / PAS_GRILLE)) + 1u;
        let col = f32(g % colonnes);
        let ligne = f32(g / colonnes);
        centre_px = vec2<f32>(col * PAS_GRILLE, ligne * PAS_GRILLE);
        rayon_px = 1.0;
        // Plus présente au centre de l'écran, effacée vers les bords : un plan, pas un papier.
        let u = centre_px / cadre.resolution - 0.5;
        let centre = 1.0 - smoothstep(0.15, 0.7, length(u) * 1.6);
        intensite = (0.05 + 0.12 * centre) * cadre.attenuation;
    }

    let position_px = centre_px + coin * rayon_px;
    let ndc = vec2<f32>(
        position_px.x / cadre.resolution.x * 2.0 - 1.0,
        1.0 - position_px.y / cadre.resolution.y * 2.0,
    );

    var sortie: Sortie;
    sortie.position = vec4<f32>(ndc, 0.0, 1.0);
    sortie.centre = coin;
    sortie.couleur = vec4<f32>(teinte, intensite);
    return sortie;
}

@fragment
fn fs(entree: Sortie) -> @location(0) vec4<f32> {
    // Une particule est un point doux, pas un carré. Le carré se verrait immédiatement.
    let d = length(entree.centre);
    let masque = 1.0 - smoothstep(0.0, 1.0, d);
    let a = entree.couleur.a * masque * masque;
    // Prémultiplié : le mélange additif qui suit attend une couleur déjà pondérée.
    return vec4<f32>(entree.couleur.rgb * a, a);
}
