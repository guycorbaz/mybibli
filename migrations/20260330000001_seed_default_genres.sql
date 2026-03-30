-- Seed default genres for title creation
-- Uses WHERE NOT EXISTS for idempotency (safe to re-run)

INSERT INTO genres (name) SELECT 'Roman' FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM genres WHERE name = 'Roman');
INSERT INTO genres (name) SELECT 'BD' FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM genres WHERE name = 'BD');
INSERT INTO genres (name) SELECT 'Science-Fiction' FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM genres WHERE name = 'Science-Fiction');
INSERT INTO genres (name) SELECT 'Policier' FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM genres WHERE name = 'Policier');
INSERT INTO genres (name) SELECT 'Jeunesse' FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM genres WHERE name = 'Jeunesse');
INSERT INTO genres (name) SELECT 'Musique' FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM genres WHERE name = 'Musique');
INSERT INTO genres (name) SELECT 'Film' FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM genres WHERE name = 'Film');
INSERT INTO genres (name) SELECT 'Documentaire' FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM genres WHERE name = 'Documentaire');
INSERT INTO genres (name) SELECT 'Revue' FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM genres WHERE name = 'Revue');
INSERT INTO genres (name) SELECT 'Rapport' FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM genres WHERE name = 'Rapport');
INSERT INTO genres (name) SELECT 'Non classé' FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM genres WHERE name = 'Non classé');
