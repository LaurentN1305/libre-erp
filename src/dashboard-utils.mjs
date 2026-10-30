/** Les types internes des échéances ne sont pas des libellés utilisateur. */
export function deadlineLabel(kind) {
  return kind.replace(/^(work|equipment|contract|document|manual):/, '').split('|scope:')[0];
}
