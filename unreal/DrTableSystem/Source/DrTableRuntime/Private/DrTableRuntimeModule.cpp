// Copyright ldnjin81. All Rights Reserved.

#include "HAL/IConsoleManager.h"
#include "Modules/ModuleManager.h"
#include "DrTableRegistry.h"
#include "DrTableSettings.h"

UDrTableSettings::UDrTableSettings()
	: AssetRoot(TEXT("/Game/Data"))
{
}

namespace DrTable::Private
{
	static FAutoConsoleCommandWithOutputDevice StatusCommand(
		TEXT("DrTable.Status"),
		TEXT("Lists the registered tables and how many rows each has loaded."),
		FConsoleCommandWithOutputDeviceDelegate::CreateLambda([](FOutputDevice& Out)
		{
			if (const UDrTableRegistry* Registry = UDrTableRegistry::Get())
			{
				Registry->DumpStatus(Out);
			}
			else
			{
				Out.Log(TEXT("DrTable: no active registry."));
			}
		}));

	static FAutoConsoleCommand ReloadCommand(
		TEXT("DrTable.Reload"),
		TEXT("Reloads every registered table from its baked asset. Row pointers obtained before the reload become invalid."),
		FConsoleCommandDelegate::CreateLambda([]()
		{
			if (UDrTableRegistry* Registry = UDrTableRegistry::Get())
			{
				Registry->RegisterAutoTables();
				Registry->ReloadAllTables();
			}
		}));
} // namespace DrTable::Private

IMPLEMENT_MODULE(FDefaultModuleImpl, DrTableRuntime)
