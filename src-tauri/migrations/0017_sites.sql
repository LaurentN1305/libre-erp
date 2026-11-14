-- Sites : un « bâtiment » peut aussi être un équipement sportif ou une aire de jeux,
-- éventuellement rattaché à un bâtiment. Le rattachement est une simple référence :
-- il ne bloque ni l'export ni l'import d'un site isolé.
ALTER TABLE buildings ADD COLUMN nature TEXT NOT NULL DEFAULT 'building' CHECK(nature IN('building','sports','playground'));
ALTER TABLE buildings ADD COLUMN parent_site_id TEXT;
INSERT OR IGNORE INTO equipment_categories VALUES('e23e3de8-e4ce-540b-9bbd-41ee3b878c57',NULL,'SPORT','Équipements sportifs');
INSERT OR IGNORE INTO equipment_categories VALUES('f05e6e34-795a-55ad-a884-69dc724caa33',(SELECT id FROM equipment_categories WHERE code='SPORT'),'SPORT-01','Buts (football, handball, hockey)');
INSERT OR IGNORE INTO equipment_categories VALUES('fb219ff8-af6f-5b75-9159-5df245b872b6',(SELECT id FROM equipment_categories WHERE code='SPORT'),'SPORT-02','Paniers de basket');
INSERT OR IGNORE INTO equipment_categories VALUES('3f98dfe4-b50c-5396-bc86-2dbefc7dab37',(SELECT id FROM equipment_categories WHERE code='SPORT'),'SPORT-03','Autres équipements sportifs');
INSERT OR IGNORE INTO equipment_categories VALUES('84f33921-5eb1-5aa4-b1e6-8c811faa6ccf',NULL,'PLAY','Aires de jeux');
INSERT OR IGNORE INTO equipment_categories VALUES('27863848-13c9-5a49-9325-b4b73ccae519',(SELECT id FROM equipment_categories WHERE code='PLAY'),'PLAY-01','Jeux (toboggans, balançoires, structures)');
INSERT OR IGNORE INTO equipment_categories VALUES('87cc9587-17cc-5a70-9b40-4b6005fcb716',(SELECT id FROM equipment_categories WHERE code='PLAY'),'PLAY-02','Sols amortissants');
INSERT OR IGNORE INTO equipment_categories VALUES('e14c2457-1dac-5812-b57b-2647e00d3a0e',(SELECT id FROM equipment_categories WHERE code='PLAY'),'PLAY-03','Clôtures, portillons et mobilier');
