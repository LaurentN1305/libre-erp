import { useFeatures } from './Features';

export type BuildingTab = 'summary' | 'register' | 'documents' | 'inventory' | 'contracts' | 'alerts';
export type BuildingAccess = { id: string; name: string; register_read?: boolean; documents_read?: boolean; inventory_read?: boolean; contracts_read?: boolean; alerts_read?: boolean };

/** Onglets d'un bâtiment : la même barre sur le résumé et sur chaque rubrique. */
export function BuildingTabs({ building, active, showTitle = false, onSelect, onClose }: { building: BuildingAccess; active: BuildingTab; showTitle?: boolean; onSelect: (tab: BuildingTab) => void; onClose: () => void }) {
  const features = useFeatures();
  const tabs: [BuildingTab, string, boolean][] = [
    ['summary', 'Résumé', true],
    ['register', 'Registre de sécurité', !!building.register_read],
    ['documents', 'Documents', features.documents && !!building.documents_read],
    ['inventory', 'Équipements', features.inventory && !!building.inventory_read],
    ['contracts', 'Contrats', features.contracts && !!building.contracts_read],
    ['alerts', 'Alertes', features.alerts && !!building.alerts_read],
  ];
  return <div className="building-tabs-block">
    <div className="building-tabs-title"><button className="link-back" onClick={onClose}>← Tous les sites</button>{showTitle && <h2>{building.name}</h2>}</div>
    <nav className="tabs building-tabs" aria-label={`Rubriques de ${building.name}`}>{tabs.filter(t => t[2]).map(([id, label]) => <button key={id} className={id === active ? 'selected' : ''} aria-current={id === active ? 'page' : undefined} onClick={() => { if (id !== active) onSelect(id); }}>{label}</button>)}</nav>
  </div>;
}
