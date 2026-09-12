// Le champ de courants.
//
// Chaque tâche est un filament qui traverse l'écran. Les particules qui le composent avancent à la
// vitesse de la tâche : un agent qui travaille vite fait filer son courant, un agent bloqué le fige
// sur place. La clarté dit ce qui reste de budget.
//
// Rien ici n'est décoratif. Si une particule bouge, c'est qu'une étape a été franchie.

struct Cadre {
    resolution: vec2<f32>,
    temps: f32,
    attenuation: f32,
};

struct Courant {
    // Hauteur de base du filament, en fraction de l'écran.
    base: f32,
    // Amplitude de son ondulation.
    amplitude: f32,
    // Vitesse d'avance des particules.
    vitesse: f32,
    // Clarté, de 0 à 1.
    clarte: f32,
    // Décalage de phase, pour que deux courants ne se superposent pas.
    phase: f32,
    // Teinte : 0 = or, 1 = l'orange réservé à ce qui attend un humain.
    teinte: f32,
    _remplissage: vec2<f32>,
};

@group(0) @binding(0) var<uniform> cadre: Cadre;
@group(0) @binding(1) var<storage, read> courants: array<Courant>;

struct Sortie {
    @builtin(position) position: vec4<f32>,
    @location(0) centre: vec2<f32>,
    @location(1) couleur: vec4<f32>,
};

// La courbe que suit un filament. Trois harmoniques : une donne un arc, deux donnent une vague,
// trois donnent quelque chose qui ne se répète pas à l'œil.
fn courbe(x: f32, c: Courant) -> f32 {
    return c.base
        + c.amplitude * sin(x * 3.1 + c.phase)
        + c.amplitude * 0.45 * sin(x * 7.3 - c.phase * 1.7)
        + c.amplitude * 0.20 * sin(x * 13.1 + c.phase * 0.6);
}

// Un bruit bon marché et reproductible. Reproductible importe : deux rendus de la même scène au
// même instant doivent donner la même image, sans quoi rien ne se compare.
fn aleatoire(graine: f32) -> f32 {
    return fract(sin(graine * 12.9898) * 43758.5453);
}

@vertex
fn vs(
    @builtin(vertex_index) sommet: u32,
    @builtin(instance_index) instance: u32,
) -> Sortie {
    let par_courant = 700u;
    let index_courant = instance / par_courant;
    let index_particule = instance % par_courant;
    let c = courants[index_courant];

    let graine = f32(instance) + 1.0;
    let phase_particule = aleatoire(graine);
    // Les particules se répartissent autour du filament, plus denses au centre : c'est ce qui
    // donne un ruban plutôt qu'un trait.
    let ecart = (aleatoire(graine * 1.7) - 0.5);
    let lateral = ecart * ecart * ecart * 0.09;

    // L'avance. `fract` rebouclerait brutalement ; on atténue plutôt aux deux bords.
    let t = fract(phase_particule + cadre.temps * c.vitesse);
    let x = t;
    let y = courbe(x, c) + lateral;

    // Deux triangles autour du centre, dilatés en pixels pour que la taille ne dépende pas de la
    // résolution.
    let coins = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
    );
    let coin = coins[sommet];
    let rayon_px = 1.0 + aleatoire(graine * 3.3) * 2.2;

    let centre_px = vec2<f32>(x * cadre.resolution.x, y * cadre.resolution.y);
    let position_px = centre_px + coin * rayon_px;

    // Pixels vers coordonnées normalisées, l'origine en haut à gauche.
    let ndc = vec2<f32>(
        position_px.x / cadre.resolution.x * 2.0 - 1.0,
        1.0 - position_px.y / cadre.resolution.y * 2.0,
    );

    // Atténuation aux bords : un filament naît et meurt au lieu de se couper net.
    let bord = smoothstep(0.0, 0.14, t) * (1.0 - smoothstep(0.82, 1.0, t));
    // Un scintillement lent, qui donne de la profondeur sans attirer l'œil.
    let scintille = 0.72 + 0.28 * sin(cadre.temps * 1.7 + graine);

    let or = vec3<f32>(0.91, 0.71, 0.33);
    let alerte = vec3<f32>(0.85, 0.38, 0.24);
    let teinte = mix(or, alerte, c.teinte);

    let intensite = c.clarte * bord * scintille * cadre.attenuation;

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
