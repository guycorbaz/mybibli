-- Seed default volume states and contributor roles
-- Uses WHERE NOT EXISTS for idempotency (safe to re-run)

-- Volume states with loanable flags
INSERT INTO volume_states (name, is_loanable) SELECT 'Neuf', TRUE FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM volume_states WHERE name = 'Neuf');
INSERT INTO volume_states (name, is_loanable) SELECT 'Bon', TRUE FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM volume_states WHERE name = 'Bon');
INSERT INTO volume_states (name, is_loanable) SELECT 'Usé', TRUE FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM volume_states WHERE name = 'Usé');
INSERT INTO volume_states (name, is_loanable) SELECT 'Endommagé', FALSE FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM volume_states WHERE name = 'Endommagé');

-- Contributor roles
INSERT INTO contributor_roles (name) SELECT 'Auteur' FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM contributor_roles WHERE name = 'Auteur');
INSERT INTO contributor_roles (name) SELECT 'Illustrateur' FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM contributor_roles WHERE name = 'Illustrateur');
INSERT INTO contributor_roles (name) SELECT 'Traducteur' FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM contributor_roles WHERE name = 'Traducteur');
INSERT INTO contributor_roles (name) SELECT 'Réalisateur' FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM contributor_roles WHERE name = 'Réalisateur');
INSERT INTO contributor_roles (name) SELECT 'Compositeur' FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM contributor_roles WHERE name = 'Compositeur');
INSERT INTO contributor_roles (name) SELECT 'Interprète' FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM contributor_roles WHERE name = 'Interprète');
INSERT INTO contributor_roles (name) SELECT 'Scénariste' FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM contributor_roles WHERE name = 'Scénariste');
INSERT INTO contributor_roles (name) SELECT 'Coloriste' FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM contributor_roles WHERE name = 'Coloriste');
