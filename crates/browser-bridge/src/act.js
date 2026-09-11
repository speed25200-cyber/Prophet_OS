// Exécute une action typée sur un élément désigné par son identifiant sémantique.
//
// Les événements sont émis comme le ferait un humain, pour que les cadres applicatifs qui les
// écoutent réagissent normalement. Un identifiant introuvable rend une erreur nommée, jamais un
// silence : c'est ce qui distingue une action typée d'un clic à l'aveugle.
((action, noeud, valeur) => {
  const cible = noeud ? document.querySelector('[data-prophet-id="' + noeud + '"]') : null;
  if (noeud && !cible) {
    return JSON.stringify({ ok: false, error: 'NodeNotFound', detail: noeud });
  }
  try {
    switch (action) {
      case 'click': {
        if (cible.disabled) return JSON.stringify({ ok: false, error: 'Disabled', detail: noeud });
        cible.click();
        return JSON.stringify({ ok: true });
      }
      case 'set_field': {
        const balise = cible.tagName.toLowerCase();
        if (!['input', 'textarea', 'select'].includes(balise)) {
          return JSON.stringify({ ok: false, error: 'NotAField', detail: balise });
        }
        if (balise === 'select') {
          const options = Array.from(cible.options).map((o) => o.value);
          if (!options.includes(valeur)) {
            return JSON.stringify({ ok: false, error: 'ValueNotAllowed', detail: options.join(', ') });
          }
        }
        const type = (cible.getAttribute('type') || '').toLowerCase();
        if (['checkbox', 'radio'].includes(type)) {
          cible.checked = valeur === 'true' || valeur === 'coché';
        } else {
          cible.value = valeur;
        }
        cible.dispatchEvent(new Event('input', { bubbles: true }));
        cible.dispatchEvent(new Event('change', { bubbles: true }));
        return JSON.stringify({ ok: true });
      }
      case 'submit': {
        const formulaire = cible ? (cible.form || cible.closest('form')) : document.querySelector('form');
        if (!formulaire) return JSON.stringify({ ok: false, error: 'NoForm', detail: '' });
        if (typeof formulaire.requestSubmit === 'function') {
          formulaire.requestSubmit();
        } else {
          formulaire.submit();
        }
        return JSON.stringify({ ok: true });
      }
      default:
        return JSON.stringify({ ok: false, error: 'UnknownAction', detail: action });
    }
  } catch (erreur) {
    return JSON.stringify({ ok: false, error: 'ActionFailed', detail: String(erreur) });
  }
})
