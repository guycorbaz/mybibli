-- Seed default location node types
INSERT INTO location_node_types (name)
SELECT 'Room' FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM location_node_types WHERE name = 'Room');
INSERT INTO location_node_types (name)
SELECT 'Furniture' FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM location_node_types WHERE name = 'Furniture');
INSERT INTO location_node_types (name)
SELECT 'Shelf' FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM location_node_types WHERE name = 'Shelf');
INSERT INTO location_node_types (name)
SELECT 'Box' FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM location_node_types WHERE name = 'Box');
