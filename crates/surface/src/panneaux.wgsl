// Les panneaux : des rectangles à coins arrondis, avec un trait fin.
//
// Ils sont dessinés par une fonction de distance plutôt que par une géométrie découpée. Un coin
// arrondi en géométrie demande des sommets et se crénelle ; une distance donne un bord net à toute
// résolution, et permet au trait de faire exactement un pixel quel que soit l'écran.

struct Cadre {
    resolution: vec2<f32>,
    _remplissage: vec2<f32>,
};

struct Panneau {
    // Coin haut-gauche, en pixels.
    origine: vec2<f32>,
    // Largeur et hauteur, en pixels.
    taille: vec2<f32>,
    // Couleur de remplissage, prémultipliée.
    fond: vec4<f32>,
    // Couleur du trait.
    trait_: vec4<f32>,
    // Rayon des coins, en pixels.
    rayon: f32,
    // Épaisseur du trait, en pixels. Zéro pour aucun trait.
    epaisseur: f32,
    _fin: vec2<f32>,
};

@group(0) @binding(0) var<uniform> cadre: Cadre;
@group(0) @binding(1) var<storage, read> panneaux: array<Panneau>;

struct Sortie {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) index: u32,
};

@vertex
fn vs(
    @builtin(vertex_index) sommet: u32,
    @builtin(instance_index) instance: u32,
) -> Sortie {
    let p = panneaux[instance];
    let coins = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let coin = coins[sommet];
    // Un pixel de marge, pour que l'anticrénelage du bord ait de la place.
    let position_px = p.origine - 1.0 + coin * (p.taille + 2.0);

    let ndc = vec2<f32>(
        position_px.x / cadre.resolution.x * 2.0 - 1.0,
        1.0 - position_px.y / cadre.resolution.y * 2.0,
    );

    var sortie: Sortie;
    sortie.position = vec4<f32>(ndc, 0.0, 1.0);
    sortie.local = position_px - p.origine;
    sortie.index = instance;
    return sortie;
}

// Distance signée à un rectangle arrondi : négative dedans, positive dehors.
fn distance_rectangle(point: vec2<f32>, demi: vec2<f32>, rayon: f32) -> f32 {
    let q = abs(point) - demi + rayon;
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - rayon;
}

@fragment
fn fs(entree: Sortie) -> @location(0) vec4<f32> {
    let p = panneaux[entree.index];
    let demi = p.taille * 0.5;
    let d = distance_rectangle(entree.local - demi, demi, p.rayon);

    // Un pixel de transition de part et d'autre du bord : ni escalier, ni flou.
    let dedans = 1.0 - smoothstep(-0.5, 0.5, d);

    var couleur = p.fond * dedans;

    if (p.epaisseur > 0.0) {
        // Le trait suit la même distance : il est donc exactement parallèle au bord, y compris
        // dans les coins, ce qu'un contour dessiné séparément ne garantit pas.
        let bande = 1.0 - smoothstep(p.epaisseur * 0.5 - 0.5, p.epaisseur * 0.5 + 0.5, abs(d));
        let t = p.trait_ * bande;
        couleur = couleur * (1.0 - t.a) + t;
    }

    return couleur;
}
