//
//  PutioAppDelegate.h
//  put.io adder
//
//  Created by Nicolas Oelgart on 2/21/13.
//  Copyright (c) 2013 Nicolas Oelgart. All rights reserved.
//

#import <Cocoa/Cocoa.h>

@interface PutioAppDelegate : NSObject <NSApplicationDelegate>

@property (assign) IBOutlet NSWindow *window;

@property (readonly, strong, nonatomic) NSPersistentStoreCoordinator *persistentStoreCoordinator;
@property (readonly, strong, nonatomic) NSManagedObjectModel *managedObjectModel;
@property (readonly, strong, nonatomic) NSManagedObjectContext *managedObjectContext;

- (IBAction)saveAction:(id)sender;

@end
