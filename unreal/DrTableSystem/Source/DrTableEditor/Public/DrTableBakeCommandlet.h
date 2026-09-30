// Copyright ldnjin81. All Rights Reserved.

#pragma once

#include "Commandlets/Commandlet.h"
#include "DrTableBakeCommandlet.generated.h"

/**
 * Bakes the client JSON written by `drtable build` into DataAssets.
 *
 *   UnrealEditor-Cmd <Project>.uproject -run=DrTableBake -Input=<client json dir> [-Out=/Game/Data] [-Force] [-Verify]
 *
 * - Class and asset names follow the manifest (cpp_prefix, asset_name), so they always
 *   match the generated code and registration.
 * - Only tables whose asset is missing or whose schema/content hash differs are saved.
 *   Re-saving identical data would still change the binary package (GUIDs), so unchanged
 *   tables are skipped. -Force saves every table.
 * - -Verify saves nothing and fails if any asset is missing or out of date (use in CI).
 * - A table whose generated class is not compiled into the editor is an error: build the
 *   project after generating, then bake.
 */
UCLASS()
class DRTABLEEDITOR_API UDrTableBakeCommandlet : public UCommandlet
{
	GENERATED_BODY()

public:
	UDrTableBakeCommandlet();
	virtual int32 Main(const FString& Params) override;
};
