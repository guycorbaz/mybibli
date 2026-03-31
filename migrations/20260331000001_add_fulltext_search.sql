-- Add FULLTEXT index for as-you-type search across title, subtitle, description
ALTER TABLE titles ADD FULLTEXT INDEX ft_titles_search (title, subtitle, description);
