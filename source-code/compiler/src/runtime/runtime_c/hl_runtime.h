#ifndef HL_RUNTIME_H
#define HL_RUNTIME_H

#ifdef __cplusplus
extern "C" {
#endif

void hl_runtime_init(void);
void hl_runtime_shutdown(void);

void hl_print(const char *msg);
void hl_print_interp(const char *tmpl);

int  hl_run_cmd(const char *cmd, int mode);
int  hl_run_background(const char *cmd);

void hl_set_var(const char *name, const char *value);
void hl_set_var_interp(const char *name, const char *tmpl);
void hl_export_var(const char *name, const char *value);
void hl_export_var_interp(const char *name, const char *tmpl);
void hl_export_list(const char *name, const char **items, int count);

int  hl_quick(const char *name, const char *args);
int  hl_dep_check(const char *name);
int  hl_get_last_exit(void);

#ifdef __cplusplus
}
#endif

#endif /* HL_RUNTIME_H */
