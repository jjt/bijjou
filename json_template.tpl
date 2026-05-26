'{'
++ '"commit_id_short":' ++ json(self.commit_id().short())
++ ',"commit_id_shortest":{"prefix":' ++ json(self.commit_id().shortest().prefix())
                       ++ ',"rest":' ++ json(self.commit_id().shortest().rest())
                       ++ '}'
++ ',"change_id_short":' ++ json(self.change_id().short())
++ ',"change_id_shortest":{"prefix":' ++ json(self.change_id().shortest().prefix())
                       ++ ',"rest":' ++ json(self.change_id().shortest().rest())
                       ++ '}'
++ ',"description":' ++ json(self.description())
++ ',"author":{"name":' ++ json(self.author().name())
            ++ ',"timestamp":' ++ json(self.author().timestamp())
            ++ '}'
++ ',"mine":' ++ json(self.mine())
++ ',"current_working_copy":' ++ json(self.current_working_copy())
++ ',"working_copies":' ++ json(self.working_copies())
++ ',"bookmarks":' ++ json(self.bookmarks())
++ ',"local_bookmarks":' ++ json(self.local_bookmarks())
++ ',"remote_bookmarks":' ++ json(self.remote_bookmarks())
++ ',"tags":' ++ json(self.tags())
++ ',"local_tags":' ++ json(self.local_tags())
++ ',"remote_tags":' ++ json(self.remote_tags())
++ ',"divergent":' ++ json(self.divergent())
++ ',"hidden":' ++ json(self.hidden())
++ ',"change_offset":' ++ json(self.change_offset())
++ ',"immutable":' ++ json(self.immutable())
++ ',"conflict":' ++ json(self.conflict())
++ ',"empty":' ++ json(self.empty())
++ ',"root":' ++ json(self.root())
++ if(self.current_working_copy(),
       ',"diff":{"files":[' ++ self.diff().files().map(|d|
            '{"path":' ++ json(d.path())
         ++ ',"display_diff_path":' ++ json(d.display_diff_path())
         ++ ',"status":' ++ json(d.status())
         ++ ',"status_char":' ++ json(d.status_char())
         ++ ',"source":{"path":' ++ json(d.source().path())
                     ++ ',"file_type":' ++ json(d.source().file_type())
                     ++ ',"executable":' ++ json(d.source().executable())
                     ++ ',"conflict":' ++ json(d.source().conflict())
                     ++ ',"conflict_side_count":' ++ json(d.source().conflict_side_count())
                     ++ '}'
         ++ ',"target":{"path":' ++ json(d.target().path())
                     ++ ',"file_type":' ++ json(d.target().file_type())
                     ++ ',"executable":' ++ json(d.target().executable())
                     ++ ',"conflict":' ++ json(d.target().conflict())
                     ++ ',"conflict_side_count":' ++ json(d.target().conflict_side_count())
                     ++ '}'
         ++ '}').join(',') ++ ']}'
    ++ ',"conflicted_files":[' ++ self.conflicted_files().map(|f|
            '{"path":' ++ json(f.path())
         ++ ',"file_type":' ++ json(f.file_type())
         ++ ',"executable":' ++ json(f.executable())
         ++ ',"conflict":' ++ json(f.conflict())
         ++ ',"conflict_side_count":' ++ json(f.conflict_side_count())
         ++ '}').join(',') ++ ']',
       '')
++ '}'
++ "\n"
