using System;
using System.Runtime.InteropServices;
using System.Text;

public static class ThreadlineReviewCache
{
    private const int SqliteOk = 0;
    private const int SqliteRow = 100;
    private const int SqliteOpenReadOnly = 1;

    private const string FixtureQuery = @"
        SELECT count(commit_row.record_key) = 1
            AND count(CASE WHEN commit_row.record_key = rev.head_oid THEN 1 END) = 1
        FROM pr_revisions rev
        JOIN accounts account ON account.id = rev.account_id
        JOIN repositories repo ON repo.id = rev.repository_id AND repo.account_id = account.id
        JOIN items item ON item.id = rev.item_id AND item.repository_id = repo.id AND item.account_id = account.id
        JOIN pr_family_resources family ON family.revision_id = rev.id AND family.family = 'commits'
        JOIN cached_resources resource ON resource.id = family.resource_id AND resource.coverage = 2
            AND resource.account_id = account.id AND resource.item_id = item.id
            AND resource.family = 'pr:commits:' || rev.id
        JOIN cached_resource_records commit_row ON commit_row.resource_id = resource.id
        WHERE rev.item_id = ?1 AND rev.id = ?2
            AND account.host = 'github.com' AND account.provider_user_id = '41'
            AND repo.provider_repository_id = '71' AND repo.owner = 'fixture-org' AND repo.name = 'private'
            AND item.number = 42 AND item.kind = 'pull_request'
            AND rev.head_oid = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb';";

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern int sqlite3_open_v2(byte[] filename, out IntPtr database, int flags, IntPtr vfs);

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern int sqlite3_prepare_v2(IntPtr database, byte[] sql, int length, out IntPtr statement, IntPtr tail);

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern int sqlite3_bind_int64(IntPtr statement, int parameter, long value);

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern int sqlite3_step(IntPtr statement);

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern int sqlite3_column_int(IntPtr statement, int column);

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern int sqlite3_finalize(IntPtr statement);

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern int sqlite3_close(IntPtr database);

    // The driver passes only the SQLite file from its closed synthetic profile.
    public static bool HasSingleCommit(string filename, long item, long revision)
    {
        IntPtr database = IntPtr.Zero;
        IntPtr statement = IntPtr.Zero;
        try
        {
            Require(sqlite3_open_v2(Utf8(filename), out database, SqliteOpenReadOnly, IntPtr.Zero), SqliteOk);
            Require(sqlite3_prepare_v2(database, Utf8(FixtureQuery), -1, out statement, IntPtr.Zero), SqliteOk);
            Require(sqlite3_bind_int64(statement, 1, item), SqliteOk);
            Require(sqlite3_bind_int64(statement, 2, revision), SqliteOk);
            Require(sqlite3_step(statement), SqliteRow);
            return sqlite3_column_int(statement, 0) == 1;
        }
        finally
        {
            if (statement != IntPtr.Zero) sqlite3_finalize(statement);
            if (database != IntPtr.Zero) sqlite3_close(database);
        }
    }

    private static byte[] Utf8(string value)
    {
        return Encoding.UTF8.GetBytes(value + '\0');
    }

    private static void Require(int actual, int expected)
    {
        if (actual != expected)
            throw new InvalidOperationException("The read-only fixture cache check failed with SQLite code " + actual + ".");
    }
}
