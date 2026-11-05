ALTER TABLE documents ADD COLUMN document_on TEXT;
ALTER TABLE documents ADD COLUMN deleted_at TEXT;
INSERT INTO document_categories (code, label) VALUES
    ('commission_report', 'Rapport de commission de sécurité');
