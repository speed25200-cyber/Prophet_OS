// Construit l'arbre sémantique d'une page.
//
// Deux principes. D'abord, chaque élément retenu reçoit un identifiant stable
// (`data-prophet-id`) : l'agent le citera pour agir, au lieu de viser des coordonnées qu'un
// changement de mise en page invaliderait. Ensuite, on ne retient que ce qui porte du sens ou se
// manipule ; le reste de l'arbre du document est du bruit pour un agent.
(() => {
  let compteur = 0;
  const identifiant = (element) => {
    if (!element.dataset.prophetId) {
      element.dataset.prophetId = 'n' + (++compteur);
    }
    return element.dataset.prophetId;
  };

  const roleDe = (element) => {
    const balise = element.tagName.toLowerCase();
    const type = (element.getAttribute('type') || '').toLowerCase();
    if (balise === 'a' && element.hasAttribute('href')) return 'link';
    if (balise === 'button' || (balise === 'input' && ['submit', 'button', 'reset'].includes(type))) return 'button';
    if (balise === 'input' && ['checkbox', 'radio'].includes(type)) return 'toggle';
    if (balise === 'input' || balise === 'textarea') return 'field';
    if (balise === 'select') return 'select';
    if (balise === 'table') return 'table';
    if (balise === 'tr') return 'row';
    if (balise === 'td' || balise === 'th') return 'cell';
    if (balise === 'ul' || balise === 'ol') return 'list';
    if (balise === 'li') return 'item';
    if (balise === 'img') return 'image';
    if (['h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'p', 'label', 'span', 'legend'].includes(balise)) return 'text';
    if (element.getAttribute('role') === 'status' || element.getAttribute('aria-live')) return 'status';
    return 'group';
  };

  const nomDe = (element) => {
    const aria = element.getAttribute('aria-label');
    if (aria) return aria;
    if (element.id) {
      const etiquette = document.querySelector('label[for="' + CSS.escape(element.id) + '"]');
      if (etiquette) return etiquette.textContent.trim();
    }
    if (element.tagName.toLowerCase() === 'img') return element.getAttribute('alt') || '';
    const propre = Array.from(element.childNodes)
      .filter((n) => n.nodeType === Node.TEXT_NODE)
      .map((n) => n.textContent)
      .join(' ')
      .trim();
    if (propre) return propre.slice(0, 200);
    return (element.getAttribute('placeholder') || element.getAttribute('title') || '').slice(0, 200);
  };

  const valeurDe = (element) => {
    const balise = element.tagName.toLowerCase();
    const type = (element.getAttribute('type') || '').toLowerCase();
    if (balise === 'input' && ['checkbox', 'radio'].includes(type)) return element.checked ? 'coché' : 'décoché';
    if (balise === 'input' || balise === 'textarea' || balise === 'select') return element.value;
    if (balise === 'a') return element.getAttribute('href');
    return null;
  };

  const visible = (element) => {
    const style = window.getComputedStyle(element);
    if (style.display === 'none' || style.visibility === 'hidden' || style.opacity === '0') return false;
    if (element.hasAttribute('hidden') || element.getAttribute('aria-hidden') === 'true') return false;
    return element.offsetWidth > 0 || element.offsetHeight > 0 || element.getClientRects().length > 0;
  };

  const actionnable = (element) => {
    const balise = element.tagName.toLowerCase();
    if (['a', 'button', 'input', 'select', 'textarea'].includes(balise)) return !element.disabled;
    return element.hasAttribute('onclick') || element.getAttribute('role') === 'button';
  };

  const RETENUS = new Set(['link', 'button', 'field', 'toggle', 'select', 'table', 'row', 'cell', 'list', 'item', 'image', 'text', 'status']);

  const construire = (element) => {
    if (!visible(element)) return null;
    const enfants = Array.from(element.children)
      .map(construire)
      .filter((n) => n !== null);
    const role = roleDe(element);
    const nom = nomDe(element);
    const valeur = valeurDe(element);
    const peutAgir = actionnable(element);

    // Un groupe sans nom, sans valeur, non actionnable et à un seul enfant n'apporte rien :
    // on le remplace par son enfant pour raccourcir l'arbre sans perdre d'information.
    if (role === 'group' && !nom && !valeur && !peutAgir) {
      if (enfants.length === 1) return enfants[0];
      if (enfants.length === 0) return null;
    }
    if (!RETENUS.has(role) && enfants.length === 0 && !peutAgir) return null;

    const noeud = {
      id: identifiant(element),
      role: role,
      name: nom,
      actionable: peutAgir,
      disabled: !!element.disabled,
    };
    if (valeur !== null && valeur !== undefined) noeud.value = String(valeur);
    if (enfants.length) noeud.children = enfants;
    return noeud;
  };

  const racine = construire(document.body) || { id: 'root', role: 'group', name: '', actionable: false, disabled: false };
  const actif = document.activeElement;

  // Les actions disponibles se déduisent de ce que la page contient réellement.
  const actions = [{ name: 'navigate', description: "Ouvre une adresse.", args: [{ name: 'url', type: 'string', required: true }], irreversible: false, external: true }];
  const aChamp = document.querySelector('input, textarea, select');
  if (aChamp) {
    actions.push({ name: 'set_field', description: "Renseigne un champ du formulaire.", args: [{ name: 'node', type: 'string', required: true }, { name: 'value', type: 'string', required: true }], irreversible: false, external: false });
  }
  if (document.querySelector('a, button, [role=button]')) {
    actions.push({ name: 'click', description: "Active un élément.", args: [{ name: 'node', type: 'string', required: true }], irreversible: false, external: false });
  }
  if (document.querySelector('form')) {
    // Soumettre un formulaire part vers un service distant et peut déclencher un achat, une
    // réservation ou un envoi : c'est irréversible et externe, donc soumis à approbation.
    actions.push({ name: 'submit', description: "Soumet un formulaire.", args: [{ name: 'node', type: 'string', required: false }], irreversible: true, external: true });
  }

  return JSON.stringify({
    v: 0,
    app: 'prophet.browser',
    window: location.origin + location.pathname,
    title: document.title,
    version: Date.now(),
    root: racine,
    actions: actions,
    focus: actif && actif.dataset ? actif.dataset.prophetId || null : null,
  });
})()
